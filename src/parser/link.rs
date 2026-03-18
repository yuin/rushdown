extern crate alloc;
use crate::ast::{Arena, Image, Link, NodeRef, Text, TextQualifier};
use crate::parser::{
    process_delimiters, Context, InlineParser, LinkLabel, ParseStackElemData, ParseStackElemRef,
};
use crate::text::{self, block_to_bytes, block_to_value, Reader, Segment, EOS};
use crate::util::{is_blank, is_punct, is_space, to_link_reference};
use crate::{as_kind_data, matches_kind};
use alloc::string::String;

/// [`InlineParser`] for links.
#[derive(Debug, Default)]
pub struct LinkParser {}

impl LinkParser {
    /// Returns a new [`LinkParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl InlineParser for LinkParser {
    fn trigger(&self) -> &[u8] {
        b"![]"
    }

    fn parse(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut Context,
    ) -> Option<NodeRef> {
        let (line, segment) = reader.peek_line_bytes()?;
        if line.first() == Some(&b'!') {
            if line.get(1) == Some(&b'[') {
                reader.advance(1);
                ctx.push_link_bottom();
                return process_link_label_open(
                    arena,
                    reader,
                    parent_ref,
                    segment.start() + 1,
                    true,
                    ctx,
                );
            }
            return None;
        }
        if line.first() == Some(&b'[') {
            ctx.push_link_bottom();
            return process_link_label_open(arena, reader, parent_ref, segment.start(), false, ctx);
        }
        // line[0] == ']'
        let Some(last_link_label_ref) = ctx.link_labels().top() else {
            ctx.link_bottoms_mut().remove_top(arena);
            return None;
        };
        reader.advance(1);
        // CommonMark spec says:
        //  > A link label can have at most 999 characters inside the square brackets.
        if link_label_length(arena, ctx) > 999 {
            remove_link_label(arena, last_link_label_ref, false, ctx);
            ctx.link_bottoms_mut().remove_top(arena);
            return None;
        }
        let is_image = ctx.link_labels().data(last_link_label_ref).is_image();
        if !is_image
            && contains_link(
                arena,
                Some(ctx.link_labels().elem(last_link_label_ref).node_ref()),
            )
        {
            remove_link_label(arena, last_link_label_ref, false, ctx);
            ctx.link_bottoms_mut().remove_top(arena);
            return None;
        }
        let c = reader.peek_byte();
        let (l, pos) = reader.position();
        let mut link_opt: Option<Link> = None;

        // normal link
        if c == b'(' {
            link_opt = parse_link(reader);
        // reference link
        } else if c == b'[' {
            link_opt = parse_reference_link(arena, reader, last_link_label_ref, ctx);
            if link_opt.is_none() {
                remove_link_label(arena, last_link_label_ref, false, ctx);
                ctx.link_bottoms_mut().remove_top(arena);
                return None;
            }
        }

        if link_opt.is_none() {
            reader.set_position(l, pos);
            let ssegment = Segment::new(
                ctx.link_labels()
                    .elem(last_link_label_ref)
                    .index(arena)
                    .stop(),
                segment.start(),
            );
            let maybe_link_ref = ssegment.str(reader.source());
            if maybe_link_ref.len() > 999 {
                remove_link_label(arena, last_link_label_ref, false, ctx);
                ctx.link_bottoms_mut().remove_top(arena);
                return None;
            }
            let maybe_link_ref = to_link_reference(maybe_link_ref.as_bytes());
            let Some(link_ref) =
                ctx.link_reference(String::from_utf8_lossy(&maybe_link_ref).as_ref())
            else {
                remove_link_label(arena, last_link_label_ref, false, ctx);
                ctx.link_bottoms_mut().remove_top(arena);
                return None;
            };
            let ref_def = as_kind_data!(arena, link_ref, LinkReferenceDefinition);
            link_opt = Some(match ref_def.title() {
                Some(t) => Link::with_title(ref_def.destination(), t),
                None => Link::new(ref_def.destination()),
            });
        }
        let link = link_opt.expect("should parsed");
        let link_ref = if is_image {
            arena.new_node(match link.title() {
                Some(t) => Image::with_title(link.destination().clone(), t.clone()),
                None => Image::new(link.destination().clone()),
            })
        } else {
            arena.new_node(link)
        };
        #[cfg(feature = "inline-pos")]
        {
            use crate::as_type_data_mut;

            let pos = ctx
                .link_labels()
                .elem(last_link_label_ref)
                .index(arena)
                .start();
            as_type_data_mut!(arena, link_ref, Inline).set_pos(pos);
        }

        process_link_label(arena, link_ref, last_link_label_ref, ctx);
        remove_link_label(arena, last_link_label_ref, true, ctx);
        Some(link_ref)
    }

    fn close_block(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        _reader: &mut text::BlockReader,
        ctx: &mut Context,
    ) {
        ctx.link_bottoms_mut().remove_until(arena, None);
        ctx.link_labels_mut().remove_until(arena, None);
    }
}

fn parse_reference_link(
    arena: &mut Arena,
    reader: &mut text::BlockReader,
    last_ref: ParseStackElemRef,
    ctx: &mut Context,
) -> Option<Link> {
    let (_, start_pos) = reader.position();
    reader.advance(1); // skip '['
    let (l, pos) = reader.position();
    let mut closed = false;
    let mut opened = 0;
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
            opened += 1;
        } else if c == b']' {
            opened -= 1;
            if opened < 0 {
                closed = true;
                break;
            }
        }
        reader.advance(1);
    }
    if !closed {
        return None;
    }
    let segs = reader.between_current(l, pos);
    reader.advance(1); // skip ']'
    let mut maybe_link_ref = block_to_bytes(segs, reader.source());
    if is_blank(&maybe_link_ref) {
        // collapsed reference link
        maybe_link_ref = Segment::new(
            ctx.link_labels().get_elem(last_ref)?.index(arena).stop(),
            start_pos.start() - 1,
        )
        .bytes(reader.source());
    }
    // CommonMark spec says:
    //  > A link label can have at most 999 characters inside the square brackets.
    if maybe_link_ref.len() > 999 {
        return None;
    }

    let maybe_link_ref = to_link_reference(maybe_link_ref);
    let link_ref = ctx.link_reference(String::from_utf8_lossy(&maybe_link_ref).as_ref())?;
    let ref_def = as_kind_data!(arena, link_ref, LinkReferenceDefinition);

    Some(match ref_def.title() {
        Some(t) => Link::with_title(ref_def.destination(), t),
        None => Link::new(ref_def.destination()),
    })
}

fn process_link_label(
    arena: &mut Arena,
    link_ref: NodeRef,
    last_ref: ParseStackElemRef,
    ctx: &mut Context,
) {
    let bottom_ref = ctx.link_bottoms().top();
    let b = bottom_ref.and_then(|r| ctx.link_bottoms().get_data(r).cloned());
    process_delimiters(arena, b, ctx);
    let mut c_opt = arena[ctx.link_labels().elem(last_ref).node_ref()].next_sibling();
    while let Some(c) = c_opt {
        let next = arena[c].next_sibling();
        link_ref.append_child(arena, c);
        c_opt = next;
    }
}

fn parse_link(reader: &mut text::BlockReader) -> Option<Link> {
    reader.advance(1); // skip '('
    reader.skip_spaces();

    let mut destination: Option<text::Value> = None;
    let mut title: Option<text::Value> = None;
    // empty link like '[link]()'
    if reader.peek_byte() == b')' {
        reader.advance(1);
    } else {
        let d = parse_link_destination(reader)?;
        destination = Some(d);
        reader.skip_spaces();
        if reader.peek_byte() == b')' {
            reader.advance(1);
        } else {
            match parse_link_title(reader) {
                ParseLinkTitleResult::Ok(t) => title = Some(t),
                ParseLinkTitleResult::Unclosed => return None,
                ParseLinkTitleResult::None => title = None,
            }
            reader.skip_spaces();
            if reader.peek_byte() == b')' {
                reader.advance(1);
            } else {
                return None;
            }
        }
    }
    let destination = destination.unwrap_or_else(|| "".into());
    Some(match title {
        Some(t) => Link::with_title(destination, t),
        None => Link::new(destination),
    })
}

pub(super) enum ParseLinkTitleResult {
    Ok(text::Value),
    Unclosed,
    None,
}

pub(super) fn parse_link_title(reader: &mut text::BlockReader) -> ParseLinkTitleResult {
    reader.skip_spaces();
    let opener = reader.peek_byte();
    if opener != b'"' && opener != b'\'' && opener != b'(' {
        return ParseLinkTitleResult::None;
    }
    let mut closer = opener;
    if opener == b'(' {
        closer = b')';
    }
    reader.advance(1);
    let (l, pos) = reader.position();
    let mut closed = false;
    loop {
        let c = reader.peek_byte();
        if c == EOS {
            break;
        }
        if c == b'\\' {
            reader.advance(1);
            if reader.peek_byte() == closer || reader.peek_byte() == opener {
                reader.advance(1);
                continue;
            }
        }
        if c == closer {
            closed = true;
            break;
        }
        reader.advance(1);
    }

    if !closed {
        return ParseLinkTitleResult::Unclosed;
    }

    let title = block_to_value(reader.between_current(l, pos), reader.source());
    reader.advance(1); // skip a closer
    ParseLinkTitleResult::Ok(title)
}

pub(super) fn parse_link_destination(reader: &mut text::BlockReader) -> Option<text::Value> {
    reader.skip_spaces();
    let (line, segment) = reader.peek_line_bytes()?;
    if line.first() == Some(&b'<') {
        let mut i = 1;
        while i < line.len() {
            let c = line[i];
            if c == b'\\' && i < line.len() - 1 && is_punct(line[i + 1]) {
                i += 2;
                continue;
            } else if c == b'>' {
                reader.advance(i + 1);
                return Some(Segment::new(segment.start() + 1, segment.start() + i).into());
            }
            i += 1;
        }
        return None;
    }
    let mut opened = 0;
    let mut i = 0;

    while i < line.len() {
        let c = line[i];
        if c == b'\\' && i < line.len() - 1 && is_punct(line[i + 1]) {
            i += 2;
            continue;
        } else if c == b'(' {
            opened += 1;
        } else if c == b')' {
            opened -= 1;
            if opened < 0 {
                break;
            }
        } else if is_space(c) {
            break;
        }
        i += 1;
    }
    if i == 0 {
        return None;
    }
    reader.advance(i);
    Some(Segment::new(segment.start(), segment.start() + i).into())
}

fn contains_link(arena: &Arena, node_ref: Option<NodeRef>) -> bool {
    let mut node_ref_opt = node_ref;
    while let Some(nref) = node_ref_opt {
        if matches_kind!(arena, nref, Link) {
            return true;
        }
        if contains_link(arena, arena[nref].first_child()) {
            return true;
        }
        node_ref_opt = arena[nref].next_sibling();
    }
    false
}

fn process_link_label_open(
    arena: &mut Arena,
    reader: &mut text::BlockReader,
    _parent_ref: NodeRef,
    pos: usize,
    is_image: bool,
    ctx: &mut Context,
) -> Option<NodeRef> {
    let start = if is_image { pos - 1 } else { pos };
    let state = LinkLabel::new(is_image);
    let seg: Segment = (start, pos + 1).into();
    let node_ref = arena.new_node(Text::with_qualifiers(seg, TextQualifier::TEMP));
    ctx.link_labels_mut().push(state, node_ref);
    reader.advance(1);
    Some(node_ref)
}

fn remove_link_label(
    arena: &mut Arena,
    elem_ref: ParseStackElemRef,
    consumes: bool,
    ctx: &mut Context,
) {
    if let Some(elem) = ctx.link_labels_mut().get_elem_mut(elem_ref) {
        if let ParseStackElemData::LinkLabel(label) = elem.data_mut() {
            if consumes {
                label.consume();
            }
        }
    }
    ctx.link_labels_mut().remove(arena, elem_ref);
}

fn link_label_length(arena: &Arena, ctx: &Context) -> usize {
    let first = ctx.link_labels().bottom();
    let last = ctx.link_labels().top();
    if let (Some(first), Some(last)) = (first, last) {
        let first = ctx.link_labels().get_elem(first).expect("should exist");
        let last = ctx.link_labels().get_elem(last).expect("should exist");
        last.index(arena).stop() - first.index(arena).start()
    } else {
        0
    }
}
