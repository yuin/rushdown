use crate::ast::{Arena, HtmlBlock, HtmlBlockType, NodeRef};
use crate::parser::{BlockParser, Context, State};
use crate::scanner::{
    scan_html_block_close_1, scan_html_block_close_2, scan_html_block_close_3,
    scan_html_block_close_4, scan_html_block_close_5, scan_html_block_open_1,
    scan_html_block_open_2, scan_html_block_open_3, scan_html_block_open_4, scan_html_block_open_5,
    scan_html_block_open_6, scan_html_block_open_7,
};
use crate::text::Reader as _;
use crate::util::is_blank;
use crate::{as_kind_data, as_type_data, as_type_data_mut, matches_kind, text};

/// [`BlockParser`] for html blocks.
#[derive(Debug, Default)]
pub struct HtmlBlockParser {}

impl HtmlBlockParser {
    /// Returns a new [`HtmlBlockParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl BlockParser for HtmlBlockParser {
    fn trigger(&self) -> &[u8] {
        b"<"
    }

    fn open(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        let (line, segment) = reader.peek_line_bytes()?;
        let node_ref = if scan_html_block_open_1(&line).is_some() {
            arena.new_node(HtmlBlock::new(HtmlBlockType::Type1))
        } else if scan_html_block_open_2(&line).is_some() {
            arena.new_node(HtmlBlock::new(HtmlBlockType::Type2))
        } else if scan_html_block_open_3(&line).is_some() {
            arena.new_node(HtmlBlock::new(HtmlBlockType::Type3))
        } else if scan_html_block_open_4(&line).is_some() {
            arena.new_node(HtmlBlock::new(HtmlBlockType::Type4))
        } else if scan_html_block_open_5(&line).is_some() {
            arena.new_node(HtmlBlock::new(HtmlBlockType::Type5))
        } else if scan_html_block_open_6(&line).is_some() {
            arena.new_node(HtmlBlock::new(HtmlBlockType::Type6))
        } else if scan_html_block_open_7(&line).is_some() {
            // type 7 can not interrupt paragraph
            if let Some(last) = ctx.last_opened_block() {
                if matches_kind!(arena, last, Paragraph) {
                    return None;
                }
            }
            arena.new_node(HtmlBlock::new(HtmlBlockType::Type7))
        } else {
            return None;
        };
        reader.advance_to_eol();
        as_type_data_mut!(arena, node_ref, Block).append_source_line(segment);
        Some((node_ref, State::NO_CHILDREN))
    }

    fn cont(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) -> Option<State> {
        let typ = as_kind_data!(arena, node_ref, HtmlBlock).block_type();
        let (line, segment) = reader.peek_line_bytes()?;
        if typ == HtmlBlockType::Type6 || typ == HtmlBlockType::Type7 {
            if is_blank(&line) {
                reader.advance_to_eol();
                return None;
            }
        } else {
            let f = match typ {
                HtmlBlockType::Type1 => scan_html_block_close_1,
                HtmlBlockType::Type2 => scan_html_block_close_2,
                HtmlBlockType::Type3 => scan_html_block_close_3,
                HtmlBlockType::Type4 => scan_html_block_close_4,
                HtmlBlockType::Type5 => scan_html_block_close_5,
                _ => |_: &[u8]| None,
            };
            // Check if the opening line contains the closing pattern
            {
                let lines = as_type_data!(arena, node_ref, Block).source();
                if lines.len() == 1 && f(&lines.last().unwrap().bytes(reader.source())).is_some() {
                    return None;
                }
            }
            if f(&line).is_some() {
                reader.advance_to_eol();
                as_type_data_mut!(arena, node_ref, Block).append_source_line(segment);
                return None;
            }
        }

        as_type_data_mut!(arena, node_ref, Block).append_source_line(segment);
        Some(State::NO_CHILDREN)
    }

    fn close(
        &self,
        _arena: &mut Arena,
        _node_ref: NodeRef,
        _reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) {
    }

    fn can_interrupt_paragraph(&self) -> bool {
        true
    }
}
