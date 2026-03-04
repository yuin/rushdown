extern crate alloc;

use alloc::boxed::Box;
use alloc::fmt;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::ast::{Arena, Link, NodeRef, Text};
use crate::parser::ParserOptions;
use crate::parser::{Context, InlineParser};
use crate::text::Segment;
use crate::util::trim_right_length;
use crate::{
    scanner::{scan_email, scan_url_strict, scan_url_www, Scan},
    text::{self, Reader},
};

/// Options for GFM auto links.
pub struct LinkifyOptions {
    /// Allowed protocols. This defaults to "http", "https", "ftp", "mailto".
    pub allowed_protocols: Vec<String>,

    /// URL scanner.
    pub url_scanner: Box<dyn Scan>,

    /// WWW scanner.
    pub www_scanner: Box<dyn Scan>,

    /// Email scanner.
    pub email_scanner: Box<dyn Scan>,
}

impl fmt::Debug for LinkifyOptions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LinkifyOptions")
            .field("allowed_protocols", &self.allowed_protocols)
            .finish()
    }
}

impl ParserOptions for LinkifyOptions {}

impl Default for LinkifyOptions {
    fn default() -> Self {
        Self {
            allowed_protocols: vec![
                "http".to_string(),
                "https".to_string(),
                "ftp".to_string(),
                "mailto".to_string(),
            ],
            url_scanner: Box::new(scan_url_strict),
            www_scanner: Box::new(scan_url_www),
            email_scanner: Box::new(scan_email),
        }
    }
}

/// [`InlineParser`] that linkifies URLs and email addresses.
#[derive(Debug, Default)]
pub struct LinkifyParser {
    options: LinkifyOptions,
}

impl LinkifyParser {
    /// Returns a new [`LinkifyParser`].
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: LinkifyOptions) -> Self {
        Self { options }
    }
}

impl InlineParser for LinkifyParser {
    fn trigger(&self) -> &[u8] {
        b" *_~("
    }

    fn parse(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut Context,
    ) -> Option<NodeRef> {
        if ctx.is_in_link_label() {
            return None;
        }
        let (line, segment) = reader.peek_line_bytes()?;
        let mut start = 0;
        let c = line[0];
        // advance if current position is not a line head.
        if c == b' ' || c == b'*' || c == b'_' || c == b'~' || c == b'(' {
            start += 1;
        }

        let mut stop: Option<usize> = None;
        let mut protocol: Option<String> = None;
        if self
            .options
            .allowed_protocols
            .iter()
            .any(|p| line[start..].starts_with(p.as_bytes()))
        {
            stop = self
                .options
                .url_scanner
                .scan(&line[start..])
                .map(|p| start + p);
        }
        if stop.is_none() && line[start..].starts_with(b"www.") {
            stop = self
                .options
                .www_scanner
                .scan(&line[start..])
                .map(|p| start + p);
            if stop.is_some() {
                protocol = Some("http".to_string())
            }
        }

        if let Some(mut stop) = stop {
            let last_char = line[stop - 1];
            if last_char == b'.' {
                stop -= 1;
            } else if last_char == b')' {
                let mut closing = 0usize;
                let mut i = stop - 1;
                while i >= start {
                    if line[i] == b')' {
                        closing += 1;
                    } else if line[i] == b'(' {
                        closing = closing.saturating_sub(1);
                    }
                    if i == 0 {
                        break;
                    }
                    i -= 1;
                }
                if closing > 0 {
                    stop -= closing;
                }
            } else if last_char == b';' {
                let mut i = stop - 2;
                while i >= start {
                    if line[i].is_ascii_alphanumeric() {
                        if i == 0 {
                            break;
                        }
                        i -= 1;
                        continue;
                    }
                    break;
                }
                if i != stop - 2 && line[i] == b'&' {
                    stop = i;
                }
            }
            stop -= trim_right_length(&line[..stop], b"?!.,:*_~");

            let label = &line[start..stop];
            let seg: Segment = (segment.start() + start, segment.start() + stop).into();
            let dest: text::Value = match protocol {
                Some(mut p) => {
                    p.push_str("://");
                    p.push_str(unsafe { core::str::from_utf8_unchecked(label) });
                    p.into()
                }
                None => seg.into(),
            };
            let auto_link_ref = arena.new_node(Link::auto(dest, seg));
            let text_ref =
                arena.new_node(Text::new(unsafe { core::str::from_utf8_unchecked(label) }));
            auto_link_ref.append_child_fast(arena, text_ref);
            if start != 0 {
                parent_ref.merge_or_append_text_segment(
                    arena,
                    (segment.start(), segment.start() + start).into(),
                );
            }
            reader.advance(start + (stop - start));
            return Some(auto_link_ref);
        }

        let mut stop = self
            .options
            .email_scanner
            .scan(&line[start..])
            .map(|p| start + p)?;
        let at = memchr::memchr(b'@', &line)?;
        memchr::memchr(b'.', &line[at..stop])?;
        if line[stop - 1] == b'.' {
            stop -= 1;
        }
        if stop < line.len() {
            let next = line[stop];
            if next == b'-' || next == b'_' {
                return None;
            }
        }
        stop -= trim_right_length(&line[..stop], b"?!.,:*_~");
        if start != 0 {
            parent_ref.merge_or_append_text_segment(
                arena,
                (segment.start(), segment.start() + start).into(),
            );
        }
        let label = unsafe { core::str::from_utf8_unchecked(&line[start..stop]) };
        let seg: Segment = (segment.start() + start, segment.start() + stop).into();
        let mut dest = "mailto:".to_string();
        dest.push_str(label);
        let auto_link_ref = arena.new_node(Link::auto(dest, seg));
        let text_ref = arena.new_node(Text::new(label));
        auto_link_ref.append_child_fast(arena, text_ref);
        reader.advance(start + (stop - start));
        Some(auto_link_ref)
    }
}
