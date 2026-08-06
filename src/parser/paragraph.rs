use crate::{
    as_type_data_mut,
    ast::{Arena, NodeRef, Paragraph},
    parser::{BlockParser, Context, State},
    text,
    text::Reader as _,
};

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
        let segment = reader.peek_line_segment()?;
        if segment.is_blank(reader.source()) {
            return None;
        }
        let node_ref = arena.new_node(Paragraph::new());
        as_type_data_mut!(arena, node_ref, Block)
            .append_source_line(segment.trim_left_space(reader.source()));

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
        let segment = reader.peek_line_segment()?;
        if segment.is_blank(reader.source()) {
            return None;
        }
        let block = as_type_data_mut!(arena, node_ref, Block);
        block.append_source_line(segment.trim_left_space(reader.source()));
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
        // trim trailing spaces
        if let Some(last) = block.source().last() {
            block.replace_source_line(
                block.source().len() - 1,
                last.trim_right_space(reader.source()),
            );
        }
        // remove empty paragraph
        if block.source().is_empty() {
            node_ref.delete(arena);
        }
    }
}
