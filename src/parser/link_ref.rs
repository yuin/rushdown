extern crate alloc;

use alloc::vec::Vec;

use crate::ast::{Arena, LinkReferenceDefinition, NodeRef};
use crate::parser::{
    parse_link_destination, parse_link_title, Context, ParagraphTransformer, ParseLinkTitleResult,
};
use crate::text::{self, Reader, ValuesExt, EOS};
use crate::util::indent_width;
use crate::util::is_blank;
use crate::{as_kind_data, as_type_data_mut};

/// [`ParagraphTransformer`] that extracts link reference definitions from paragraphs.
#[derive(Debug, Default)]
pub struct LinkReferenceParagraphTransformer {}

impl LinkReferenceParagraphTransformer {
    /// Returns a new [`LinkReferenceParagraphTransformer`].
    pub fn new() -> Self {
        Self {}
    }
}

impl ParagraphTransformer for LinkReferenceParagraphTransformer {
    fn transform(
        &self,
        arena: &mut Arena,
        paragraph_ref: NodeRef,
        reader: &mut text::BasicReader,
        context: &mut Context,
    ) {
        let mut lines = as_type_data_mut!(arena, paragraph_ref, Block).take_source();
        let mut block = text::BlockReader::new(reader.source(), &lines);
        let mut removes = Vec::<(NodeRef, usize, usize)>::new();
        while let Some((link_ref, start, mut end)) = parse_link_reference_definition(
            arena,
            arena[paragraph_ref].parent().unwrap(),
            paragraph_ref,
            &mut block,
            context,
        ) {
            if start == end {
                end += 1
            }
            removes.push((link_ref, start, end));
        }
        let mut offset = 0;
        for (link_ref, start, end) in removes {
            if lines.is_empty() {
                break;
            }
            let ll = &lines[start - offset..end - offset];
            as_type_data_mut!(arena, link_ref, Block).append_source_lines(ll);

            let s1 = lines[end - offset..].to_vec();
            lines = lines[..start - offset].to_vec();
            lines.extend_from_slice(&s1);
            offset = end;
        }
        if lines.is_empty() {
            let bl = as_type_data_mut!(arena, paragraph_ref, Block).has_blank_previous_line();
            if let Some(p) = arena[paragraph_ref].parent() {
                if let Some(n) = arena[p].next_sibling() {
                    as_type_data_mut!(arena, n, Block).set_blank_previous_line(bl);
                }
            }
            paragraph_ref.delete(arena);
        } else {
            as_type_data_mut!(arena, paragraph_ref, Block).put_back_source(lines);
        }
    }
}

fn parse_link_reference_definition<'a>(
    arena: &mut Arena,
    parent: NodeRef,
    paragraph: NodeRef,
    reader: &mut text::BlockReader<'a>,
    ctx: &mut Context,
) -> Option<(NodeRef, usize, usize)> {
    reader.skip_spaces();
    let (line, _) = reader.peek_line_bytes()?;
    let (start_line, _) = reader.position();
    let (width, mut pos) = indent_width(&line, 0);
    if width > 3 {
        return None;
    }
    if width != 0 {
        pos += 1;
    }
    if line.get(pos) != Some(&b'[') {
        return None;
    }
    reader.advance(pos + 1);
    let (l, pos) = reader.position();
    let mut closed = false;
    loop {
        let c = reader.peek_byte();
        if c == EOS {
            break;
        }
        if c == b'\\' {
            reader.advance(1);
            if reader.peek_byte() == b']' || reader.peek_byte() == b'[' {
                reader.advance(1);
                continue;
            }
        }
        if c == b'[' {
            return None;
        } else if c == b']' {
            closed = true;
            break;
        }
        reader.advance(1);
    }
    if !closed {
        return None;
    }

    let label = reader.between_current(l, pos);
    reader.advance(1); // skip a closer
    if is_blank(&label.bytes(reader.source())) {
        return None;
    }
    if reader.peek_byte() != b':' {
        return None;
    }
    reader.advance(1);
    reader.skip_spaces();
    let destination = parse_link_destination(reader)?;

    let l = reader.peek_line_bytes();
    let (line, _) = reader.position();
    let has_newline = l.is_none_or(|(line, _)| is_blank(&line));
    let has_spaces = reader.skip_spaces() > 0;
    let opener = reader.peek_byte();
    if opener != b'"' && opener != b'\'' && opener != b'(' {
        if !has_newline {
            return None;
        }
        let link_ref = arena.new_node(LinkReferenceDefinition::new(label, destination));
        parent.insert_before(arena, paragraph, link_ref);
        ctx.add_link_reference(
            as_kind_data!(arena, link_ref, LinkReferenceDefinition).label_str(reader.source()),
            link_ref,
        );
        let (end_line, _) = reader.position();
        return Some((
            link_ref,
            start_line,
            end_line + if end_line != line { 0 } else { 1 },
        ));
    }

    if !has_spaces {
        return None;
    }
    let title_result = parse_link_title(reader);
    let empty_title = matches!(title_result, ParseLinkTitleResult::Ok(_) if reader.peek_line_bytes().is_some_and(|l| !is_blank(&l.0)))
        || matches!(title_result, ParseLinkTitleResult::None)
        || matches!(title_result, ParseLinkTitleResult::Unclosed);
    if empty_title {
        if !has_newline {
            return None;
        }
        let link_ref = arena.new_node(LinkReferenceDefinition::new(label, destination));
        parent.insert_before(arena, paragraph, link_ref);
        ctx.add_link_reference(
            as_kind_data!(arena, link_ref, LinkReferenceDefinition).label_str(reader.source()),
            link_ref,
        );
        let (end_line, _) = reader.position();
        return Some((link_ref, start_line, end_line));
    }
    if let ParseLinkTitleResult::Ok(t) = title_result {
        let link_ref = arena.new_node(LinkReferenceDefinition::with_title(label, destination, t));
        parent.insert_before(arena, paragraph, link_ref);
        ctx.add_link_reference(
            as_kind_data!(arena, link_ref, LinkReferenceDefinition).label_str(reader.source()),
            link_ref,
        );
        let (end_line, _) = reader.position();
        Some((link_ref, start_line, end_line + 1))
    } else {
        None
    }
}
