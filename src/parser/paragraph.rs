use crate::as_type_data_mut;
use crate::ast::{Arena, NodeRef, Paragraph};
use crate::parser::{BlockParser, Context, State};
use crate::text;
use crate::text::Reader as _;
use crate::util::is_blank;

/// [`BlockParser`] for paragraphs.
#[derive(Debug, Default)]
pub struct ParagraphParser {}

impl ParagraphParser {
    /// Returns a new [`ParagraphParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl BlockParser for ParagraphParser {
    fn trigger(&self) -> &[u8] {
        &[]
    }

    fn open(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        let (_, segment) = reader.peek_line_bytes()?;
        let segment = segment.trim_left_space(reader.source());
        if segment.is_empty() {
            return None;
        }
        let node_ref = arena.new_node(Paragraph::new());
        as_type_data_mut!(arena, node_ref, Block).append_line(segment);

        reader.advance_to_eol();
        Some((node_ref, State::NO_CHILDREN))
    }

    fn cont(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) -> Option<State> {
        let (line, segment) = reader.peek_line_bytes()?;
        if is_blank(&line) {
            return None;
        }
        let block = as_type_data_mut!(arena, node_ref, Block);
        // We do not trim leading spaces here
        // ParagraphTransformer may need them
        block.append_line(segment);
        reader.advance_to_eol();
        Some(State::NO_CHILDREN)
    }

    fn close(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) {
        let block = as_type_data_mut!(arena, node_ref, Block);
        // trim leading spaces
        for i in 0..block.lines().len() {
            let line = block.lines()[i];
            block.replace_line(i, line.trim_left_space(reader.source()));
        }
        // trim trailing spaces
        if let Some(last) = block.lines().last() {
            block.replace_line(
                block.lines().len() - 1,
                last.trim_right_space(reader.source()),
            );
        }
        // remove empty paragraph
        if block.lines().is_empty() {
            node_ref.delete(arena);
        }
    }
}
