use core::cmp::max;

use crate::ast::{Arena, CodeBlock, CodeBlockType, FenceData, NodeRef};
use crate::parser::{BlockParser, Context, State};
use crate::text::{Reader as _, Segment};
use crate::util::{
    indent_position, indent_position_padding, indent_width, is_blank, trim_left_space_length,
    trim_right_space_length,
};
use crate::{as_kind_data, text};
use crate::{as_kind_data_mut, as_type_data_mut};

/// [`BlockParser`] for indented code blocks.
#[derive(Debug, Default)]
pub struct IndentedCodeBlockParser {}

impl IndentedCodeBlockParser {
    /// Returns a new [`IndentedCodeBlockParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl BlockParser for IndentedCodeBlockParser {
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
        let (line, _) = reader.peek_line_bytes()?;
        if is_blank(&line) {
            return None;
        }
        let (pos, padding) = indent_position(line.as_ref(), reader.line_offset(), 4)?;
        let code_block_ref = arena.new_node(CodeBlock::new(CodeBlockType::Indented, None));
        reader.advance_and_set_padding(pos, padding);
        let (_, mut segment) = reader.peek_line_bytes().unwrap();
        // if code block line starts with a tab, keep a tab as it is.
        if segment.padding() != 0 {
            segment = preserve_leading_tab_in_code_block(&segment, reader, 0);
        }
        segment = segment.with_force_newline(true);
        as_type_data_mut!(arena, code_block_ref, Block).append_line(segment);
        reader.advance_to_eol();
        Some((code_block_ref, State::NO_CHILDREN))
    }

    fn cont(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) -> Option<State> {
        let (line, mut segment) = reader.peek_line_bytes()?;
        if is_blank(&line) {
            let block = as_type_data_mut!(arena, node_ref, Block);
            block.append_line(segment.trim_left_space_width(4, reader.source()));
            return Some(State::NO_CHILDREN);
        }
        let (pos, padding) = indent_position(line.as_ref(), reader.line_offset(), 4)?;
        reader.advance_and_set_padding(pos, padding);
        let (_, seg) = reader.peek_line_bytes().unwrap();
        segment = seg;
        // if code block line starts with a tab, keep a tab as it is.
        if segment.padding() != 0 {
            segment = preserve_leading_tab_in_code_block(&segment, reader, 0);
        }
        segment = segment.with_force_newline(true);
        as_type_data_mut!(arena, node_ref, Block).append_line(segment);
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

        // trim trailing blank lines
        let mut lines = block.take_lines();
        let mut length = lines.len() - 1;
        let source = reader.source();
        while length != 0 {
            if lines[length].is_blank(source) {
                length -= 1;
            } else {
                break;
            }
        }
        lines.truncate(length + 1);
        block.put_back_lines(lines);
    }

    fn can_accept_indented_line(&self) -> bool {
        true
    }
}

/// [`BlockParser`] for fenced code blocks.
#[derive(Debug, Default)]
pub struct FencedCodeBlockParser {}

impl FencedCodeBlockParser {
    /// Returns a new [`FencedCodeBlockParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl BlockParser for FencedCodeBlockParser {
    fn trigger(&self) -> &[u8] {
        b"~`"
    }

    fn open(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        let (line, segment) = reader.peek_line_bytes()?;
        let pos = ctx.block_offset()?;
        let mut fdata = FenceData {
            indent: pos,
            char: line[pos],
            length: 0,
        };
        let mut i = pos;
        while i < line.len() && line[i] == fdata.char {
            i += 1;
        }
        fdata.length = i - pos;
        if fdata.length < 3 {
            return None;
        }
        let mut info: Option<text::Value> = None;
        if i < line.len() - 1 {
            let rest = &line[i..];
            let left = trim_left_space_length(rest);
            let right = trim_right_space_length(rest);
            if left < rest.len() - right {
                let info_start = segment.start() - segment.padding() + i + left;
                let info_stop = segment.stop() - right;
                let value = &rest[left..rest.len() - right];
                if fdata.char == b'`' && value.contains(&b'`') {
                    return None;
                } else if info_start != info_stop {
                    info = Some((info_start, info_stop).into());
                }
            }
        }
        let node_ref = arena.new_node(CodeBlock::new(CodeBlockType::Fenced, info));
        as_kind_data_mut!(arena, node_ref, CodeBlock).set_fence_data(fdata);
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
        let fdata = as_kind_data!(arena, node_ref, CodeBlock)
            .fence_data()
            .unwrap();
        let (w, pos) = indent_width(&line, reader.line_offset());
        if w < 4 {
            let mut i = pos;
            while i < line.len() && line[i] == fdata.char {
                i += 1;
            }
            let length = i - pos;
            if length >= fdata.length && is_blank(&line[i..]) {
                reader.advance_to_eol();
                return None;
            }
        }
        let (pos, padding) =
            indent_position_padding(&line, reader.line_offset(), segment.padding(), fdata.indent)
                .or_else(|| {
                Some((
                    max(0usize, indent_width(&line, 0).1 - segment.padding()),
                    0usize,
                ))
            })?;
        let mut seg: Segment = (segment.start() + pos, segment.stop(), padding).into();
        seg = seg.with_force_newline(true);
        // if code block line starts with a tab, keep a tab as it is.
        if padding != 0 {
            seg = preserve_leading_tab_in_code_block(&seg, reader, 0);
        }
        as_type_data_mut!(arena, node_ref, Block).append_line(seg);
        reader.advance_to_eol();
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

fn preserve_leading_tab_in_code_block(
    segment: &text::Segment,
    reader: &mut text::BasicReader,
    indent: usize,
) -> text::Segment {
    let offset_with_padding = reader.line_offset() + indent;
    let (sl, ss) = reader.position();
    reader.set_position(sl, text::Segment::new(ss.start() - 1, ss.stop()));
    if offset_with_padding == reader.line_offset() {
        let new_segment = segment.with_padding(0).with_start(segment.start() - 1);
        reader.set_position(sl, ss);
        return new_segment;
    }
    reader.set_position(sl, ss);
    *segment
}
