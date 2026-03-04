use crate::ast::{Arena, Blockquote, NodeRef};
use crate::parser::{BlockParser, Context, State};
use crate::text;
use crate::text::Reader as _;
use crate::util::{indent_width, tab_width};

/// [`BlockParser`] for blockquotes.
#[derive(Debug, Default)]
pub struct BlockquoteParser {}

impl BlockquoteParser {
    /// Returns a new [`BlockquoteParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

fn process(reader: &mut text::BasicReader) -> Option<()> {
    let (line, _) = reader.peek_line_bytes()?;
    let (_, mut pos) = indent_width(&line, reader.line_offset());
    if pos >= line.len() || line[pos] != b'>' {
        return None;
    }
    pos += 1;
    if pos >= line.len() || line[pos] == b'\n' {
        reader.advance(pos);
        return Some(());
    }
    reader.advance(pos);
    if line[pos] == b' ' || line[pos] == b'\t' {
        let mut padding = 0;
        if line[pos] == b'\t' {
            padding = tab_width(reader.line_offset()) - 1;
        }
        reader.advance_and_set_padding(1, padding);
    }
    Some(())
}

impl BlockParser for BlockquoteParser {
    fn trigger(&self) -> &[u8] {
        b">"
    }

    fn open(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        process(reader)?;
        Some((arena.new_node(Blockquote::new()), State::HAS_CHILDREN))
    }

    fn cont(
        &self,
        _arena: &mut Arena,
        _node_ref: NodeRef,
        reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) -> Option<State> {
        process(reader)?;
        Some(State::HAS_CHILDREN)
    }

    fn can_interrupt_paragraph(&self) -> bool {
        true
    }
}
