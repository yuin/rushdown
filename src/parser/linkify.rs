extern crate alloc;

use alloc::{
    boxed::Box,
    fmt,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::{
    ast::{Arena, Link, NodeRef, Text},
    parser::{Context, InlineParser, ParserOptions},
    scanner::{scan_email, scan_protocol_url, scan_www_url, Scan},
    text::{self, Reader, Segment},
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
            url_scanner: Box::new(scan_protocol_url),
            www_scanner: Box::new(scan_www_url),
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkKind {
    Url,
    Www,
    Email,
}

impl LinkifyParser {
    fn scan(&self, line: &[u8]) -> Option<(usize, LinkKind)> {
        let mut found = None;
        if self
            .options
            .allowed_protocols
            .iter()
            .any(|p| line.starts_with(p.as_bytes()))
        {
            found = self
                .options
                .url_scanner
                .scan(line)
                .map(|stop| (stop, LinkKind::Url));
        }
        if found.is_none() && line.starts_with(b"www.") {
            found = self
                .options
                .www_scanner
                .scan(line)
                .map(|stop| (stop, LinkKind::Www));
        }
        let (mut stop, kind) = match found {
            Some(found) => found,
            None => {
                let stop = self.options.email_scanner.scan(line)?;
                let at = memchr::memchr(b'@', &line[..stop])?;
                memchr::memchr(b'.', &line[at..stop])?;
                (stop, LinkKind::Email)
            }
        };
        if stop == 0 {
            return None;
        }
        if kind == LinkKind::Email {
            if line[stop - 1] == b'.' {
                stop -= 1;
            }
            if stop < line.len() && matches!(line[stop], b'-' | b'_') {
                return None;
            }
        }
        Some((stop, kind))
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
        let (full_line, segment) = reader.peek_line_bytes()?;
        let mut start = 0;
        // advance if current position is not a line head.
        if matches!(full_line[0], b' ' | b'*' | b'_' | b'~' | b'(') {
            start = 1;
        }
        let line = &full_line[start..];
        let (stop, kind) = self.scan(line)?;

        // Strip the trailing punctuation. A ')' is a part of the link only if
        // it is balanced with a '(' inside of the link, and a ';' only if it
        // terminates an entity reference.
        let opening = line[..stop].iter().filter(|&&c| c == b'(').count();
        let mut closing = line[..stop].iter().filter(|&&c| c == b')').count();
        let mut i = stop;
        while i > 1 {
            match line[i - 1] {
                b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~' | b'\'' | b'"' => i -= 1,
                b')' => {
                    if closing <= opening {
                        break;
                    }
                    closing -= 1;
                    i -= 1;
                }
                b';' => {
                    let mut j = i - 2;
                    while j > 0 && line[j].is_ascii_alphanumeric() {
                        j -= 1;
                    }
                    if j == i - 2 || line[j] != b'&' {
                        break;
                    }
                    i = j;
                }
                _ => break,
            }
        }

        let label = unsafe { core::str::from_utf8_unchecked(&line[..i]) };
        let seg: Segment = (segment.start() + start, segment.start() + start + i).into();
        let dest: text::Value = match kind {
            LinkKind::Url => seg.into(),
            LinkKind::Www => {
                let mut dest = "http://".to_string();
                dest.push_str(label);
                dest.into()
            }
            LinkKind::Email => {
                let mut dest = "mailto:".to_string();
                dest.push_str(label);
                dest.into()
            }
        };
        let auto_link_ref = arena.new_node(Link::auto(dest, seg));
        let text_ref = arena.new_node(Text::new(label));
        auto_link_ref.append_child_fast(arena, text_ref);
        if start != 0 {
            parent_ref.merge_or_append_text(
                arena,
                (segment.start(), segment.start() + start).into(),
                reader.decoder(),
            );
        }
        reader.advance(start + i);
        Some(auto_link_ref)
    }
}
