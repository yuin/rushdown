extern crate alloc;

use crate::context::{ContextKey, ContextKeyRegistry, NodeRefValue};
#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;

use alloc::rc::Rc;
use alloc::string::ToString;
use alloc::vec::Vec;

use core::cell::RefCell;
use core::cmp::min;

use crate::ast::{Arena, Attributes, Heading, NodeRef};
use crate::parser::{parse_attributes, BlockParser, Context, Options, State};
use crate::text::{BlockReader, Reader as _, Segment, EOS};
use crate::util::{
    is_blank, is_punct, is_space, trim_left_length, trim_left_space_length, trim_right_space_length,
};
use crate::{as_type_data, as_type_data_mut, matches_kind, text};

/// [`BlockParser`] for ATX headings.
#[derive(Debug, Default)]
pub struct AtxHeadingParser {
    options: Options,
}

impl AtxHeadingParser {
    /// Returns a new [`AtxHeadingParser`].
    pub fn new(options: Options) -> Self {
        Self { options }
    }
}

impl BlockParser for AtxHeadingParser {
    fn trigger(&self) -> &[u8] {
        b"#"
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
        let i = pos + trim_left_length(&line[pos..], b"#");
        let level = i - pos;
        if i == pos || level > 6 {
            return None;
        }

        // alone '#' (without a new line character)
        if i == line.len() {
            let heading_ref = arena.new_node(Heading::new(level as u8));
            return Some((heading_ref, State::NO_CHILDREN));
        }

        // needs at least one space after the '#'s for non-empty heading
        let l = trim_left_space_length(&line[i..]);
        if l == 0 {
            return None;
        }

        let start = min(i + l, line.len() - 1);
        let heading_ref = arena.new_node(Heading::new(level as u8));
        let mut hl = Segment::new(
            segment.start() + start - segment.padding(),
            segment.start() + line.len() - segment.padding(),
        );
        hl = hl.trim_right_space(reader.source());
        if hl.is_empty() {
            reader.advance_to_eol();
            return Some((heading_ref, State::NO_CHILDREN));
        }
        if self.options.attributes && memchr::memchr(b'{', &line).is_some() {
            as_type_data_mut!(arena, heading_ref, Block).append_line(hl);
            if let Some(attrs) = parse_heading_attributes(arena, heading_ref, reader, ctx) {
                arena[heading_ref].attributes_mut().extend(attrs);
                hl = *as_type_data_mut!(arena, heading_ref, Block)
                    .lines()
                    .first()
                    .unwrap();
            }
            as_type_data_mut!(arena, heading_ref, Block).remove_line(0);
        }

        // handle closing sequence of '#' characters
        let line = hl.bytes(reader.source());
        let mut stop = line.len();
        if stop != 0 {
            let mut i = stop - 1;
            while i > 0 && line[i] == b'#' {
                i -= 1;
            }
            if i == 0 && line[0] == b'#' {
                // empty headings like '### ###'
                reader.advance_to_eol();
                return Some((heading_ref, State::NO_CHILDREN));
            }
            if i != stop - 1 && is_space(line[i]) {
                stop = i;
                stop -= trim_right_space_length(&line[0..stop]);
            }
        }
        hl = hl.with_stop(hl.start() + stop);
        as_type_data_mut!(arena, heading_ref, Block).append_line(hl);
        reader.advance_to_eol();

        Some((heading_ref, State::NO_CHILDREN))
    }

    fn cont(
        &self,
        _arena: &mut Arena,
        _node_ref: NodeRef,
        _reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) -> Option<State> {
        None
    }

    fn close(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) {
        if self.options.auto_heading_ids && arena[node_ref].attributes().get("id").is_none() {
            generate_auto_heading_id(arena, node_ref, reader, ctx);
        }
    }

    fn can_interrupt_paragraph(&self) -> bool {
        true
    }
}

/// [`BlockParser`] for Setext headings.
#[derive(Debug)]
pub struct SetextHeadingParser {
    options: Options,
    temporary_paragraph: ContextKey<NodeRefValue>,
}

impl SetextHeadingParser {
    /// Returns a new [`SetextHeadingParser`].
    pub fn new(options: Options, reg: Rc<RefCell<ContextKeyRegistry>>) -> Self {
        let temporary_paragraph = reg.borrow_mut().create::<NodeRefValue>();
        Self {
            options,
            temporary_paragraph,
        }
    }
}

impl BlockParser for SetextHeadingParser {
    fn trigger(&self) -> &[u8] {
        b"-="
    }

    fn open(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        let last_ref = ctx.last_opened_block()?;
        if !matches_kind!(arena, last_ref, Paragraph)
            || arena[last_ref].parent().is_none_or(|p| p != parent_ref)
        {
            return None;
        }

        let (line, segment) = reader.peek_line_bytes()?;
        let c = matches_setext_heading_bar(&line)?;
        let level = if c == b'=' { 1 } else { 2 };
        let node_ref = arena.new_node(Heading::new(level));
        as_type_data_mut!(arena, node_ref, Block).append_line(segment);
        ctx.insert(self.temporary_paragraph, last_ref);
        Some((node_ref, State::REQUIRE_PARAGRAPH | State::NO_CHILDREN))
    }

    fn cont(
        &self,
        _arena: &mut Arena,
        _node_ref: NodeRef,
        _reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) -> Option<State> {
        None
    }

    fn close(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) {
        let Some(paragraph_ref) = ctx.remove(self.temporary_paragraph) else {
            return;
        };
        {
            let hblk = as_type_data_mut!(arena, node_ref, Block);
            hblk.remove_line(0);
        }
        if !as_type_data!(arena, paragraph_ref, Block)
            .lines()
            .is_empty()
        {
            let has_blank_previous_line: bool;
            let lines: Vec<Segment>;
            {
                let blk = as_type_data_mut!(arena, paragraph_ref, Block);
                has_blank_previous_line = blk.has_blank_previous_line();
                lines = blk.take_lines();
            }

            let hblk = as_type_data_mut!(arena, node_ref, Block);
            hblk.append_lines(&lines);
            hblk.set_blank_previous_line(has_blank_previous_line);
            if arena[paragraph_ref].parent().is_some() {
                paragraph_ref.delete(arena);
            }

            if self.options.attributes {
                if let Some(attrs) = parse_heading_attributes(arena, node_ref, reader, ctx) {
                    arena[node_ref].attributes_mut().extend(attrs);
                }
            }

            if self.options.auto_heading_ids && arena[node_ref].attributes().get("id").is_none() {
                generate_auto_heading_id(arena, node_ref, reader, ctx);
            }
        }
    }

    fn can_interrupt_paragraph(&self) -> bool {
        true
    }
}

pub(crate) fn matches_setext_heading_bar(line: &[u8]) -> Option<u8> {
    let mut start = 0;
    let mut end = line.len();
    let space = trim_left_length(&line[start..end], b" ");
    if space > 3 {
        return None;
    }
    start += space;
    let level1 = trim_left_length(&line[start..end], b"=");
    let mut c = b'=';
    let mut level2 = 0;
    if level1 == 0 {
        level2 = trim_left_length(&line[start..end], b"-");
        c = b'-';
    }
    if is_space(line[end - 1]) {
        end -= trim_right_space_length(&line[start..end]);
    }
    if (level1 > 0 && start + level1 == end) || (level2 > 0 && start + level2 == end) {
        Some(c)
    } else {
        None
    }
}

fn generate_auto_heading_id(
    arena: &mut Arena,
    node_ref: NodeRef,
    reader: &mut text::BasicReader,
    ctx: &mut Context,
) {
    let lines = as_type_data!(arena, node_ref, Block).lines();
    let heading_id = if lines.len() == 1 {
        ctx.ids_mut().generate(
            &lines.first().unwrap().str(reader.source()),
            arena[node_ref].kind_data(),
        )
    } else {
        let content = lines
            .iter()
            .map(|seg| seg.str(reader.source()))
            .collect::<Vec<_>>()
            .join(" ");
        ctx.ids_mut()
            .generate(&content, arena[node_ref].kind_data())
    };
    arena[node_ref]
        .attributes_mut()
        .insert("id".to_string(), heading_id.into());
}

fn parse_heading_attributes(
    arena: &mut Arena,
    node_ref: NodeRef,
    reader: &mut text::BasicReader,
    _ctx: &mut Context,
) -> Option<Attributes> {
    let mut attrs: Option<Attributes> = None;
    let mut consumed = 0usize;

    let seg = *{
        let lines = as_type_data!(arena, node_ref, Block).lines();
        if lines.is_empty() {
            return None;
        }
        lines.last()
    }?;
    let blk = &[seg];
    let mut lreader = BlockReader::new(reader.source(), blk);

    loop {
        let c = lreader.peek_byte();
        if c == EOS || c == b'\n' {
            break;
        }
        if c == b'\\' {
            lreader.advance(1);
            let n = lreader.peek_byte();
            if is_punct(n) {
                lreader.advance(1);
                continue;
            }
        }
        if c == b'{' {
            let (saved_line, saved_pos) = lreader.position();
            if let Some(parsed_attrs) = parse_attributes(&mut lreader) {
                let parsed = if let Some((l, _)) = lreader.peek_line_bytes() {
                    is_blank(&l)
                } else {
                    true
                };
                if parsed {
                    attrs = Some(parsed_attrs);
                    consumed = saved_pos.len();
                    break;
                }
            }
            lreader.set_position(saved_line, saved_pos);
        }
        lreader.advance(1);
    }
    if attrs.is_some() {
        let last_line = seg
            .with_stop(seg.stop() - consumed)
            .trim_right_space(reader.source());
        let blk = as_type_data_mut!(arena, node_ref, Block);
        blk.remove_line(blk.lines().len() - 1);
        blk.append_line(last_line);
    }
    attrs
}
