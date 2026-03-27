extern crate alloc;

use alloc::string::String;

use crate::ast::{Arena, Link, NodeRef, Text};
use crate::parser::{Context, InlineParser};
use crate::scanner::{scan_email, scan_url};
use crate::text::{self, Reader, Segment};

/// InlineParser that parses auto links.
#[derive(Debug, Default)]
pub struct AutoLinkParser {}

impl AutoLinkParser {
    /// Returns a new [`AutoLinkParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl InlineParser for AutoLinkParser {
    fn trigger(&self) -> &[u8] {
        b"<"
    }

    fn parse(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        _ctx: &mut Context,
    ) -> Option<NodeRef> {
        let (line, segment) = reader.peek_line_bytes()?;
        if let Some(p) = scan_url(&line[1..]) {
            reader.advance(1 + p);
            if reader.peek_byte() == b'>' {
                reader.advance(1);
                let seg: Segment = (segment.start() + 1, segment.start() + p + 1).into();
                let seg_text: Segment = (segment.start(), segment.start() + p + 2).into();
                let node_ref = arena.new_node(Link::auto(seg, seg_text));
                let text_ref = arena.new_node(Text::new(seg));
                node_ref.append_child_fast(arena, text_ref);
                return Some(node_ref);
            }
        }
        if let Some(p) = scan_email(&line[1..]) {
            reader.advance(1 + p);
            if reader.peek_byte() == b'>' {
                reader.advance(1);
                let mut s = String::new();
                s.push_str("mailto:");
                let addr = unsafe { core::str::from_utf8_unchecked(&line[1..p + 1]) };
                let addr_seg: Segment = (segment.start() + 1, segment.start() + p + 1).into();
                s.push_str(addr);
                let seg: Segment = (segment.start(), segment.start() + p + 2).into();
                let node_ref = arena.new_node(Link::auto(s, seg));
                let text_ref = arena.new_node(Text::new(addr_seg));
                node_ref.append_child_fast(arena, text_ref);
                return Some(node_ref);
            }
        }
        None
    }
}
