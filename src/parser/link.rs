extern crate alloc;
use alloc::string::String;

use crate::{
    as_kind_data,
    ast::{Arena, Image, Link, LinkReferenceKind, NodeRef, Text, TextQualifier},
    parser::{
        process_delimiters, Context, InlineParser, LinkLabel, ParseStackElemData, ParseStackElemRef,
    },
    text::{self, Reader, Segment, EOS},
    util::{is_blank, is_punct, is_space, to_link_reference},
};

const MAX_LINK_DESTINATION_NEST: usize = 32;

const SIMPLE_LINK_MAX: usize = 512;

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
                if let Some(node_ref) = parse_simple_link(arena, reader, &line, segment, true, ctx)
                {
                    return Some(node_ref);
                }
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
            if let Some(node_ref) = parse_simple_link(arena, reader, &line, segment, false, ctx) {
                return Some(node_ref);
            }
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
            && ctx
                .link_labels()
                .data(last_link_label_ref)
                .has_link_descendant()
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
            let maybe_link_ref_value = reader.between(ssegment.into());
            let maybe_link_ref = maybe_link_ref_value.str(reader.source());
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
            link_opt = Some(match ref_def.title().as_option() {
                Some(t) => Link::reference_with_title(
                    ref_def.destination(),
                    maybe_link_ref_value,
                    LinkReferenceKind::Shortcut,
                    t.clone(),
                ),
                None => Link::reference(
                    ref_def.destination(),
                    maybe_link_ref_value,
                    LinkReferenceKind::Shortcut,
                ),
            });
        }
        let link = link_opt.expect("should parsed");
        let link_ref = if is_image {
            arena.new_node(Image::from_link(link))
        } else {
            arena.new_node(link)
        };
        if let Some(p) = arena[ctx.link_labels().elem(last_link_label_ref).node_ref].pos() {
            arena[link_ref].set_pos(p);
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
    let mut link_ref_value = reader.between_current(l, pos);
    reader.advance(1); // skip ']'
    let mut maybe_link_ref = link_ref_value.bytes(reader.source());
    let mut kind = LinkReferenceKind::Full;
    if is_blank(&maybe_link_ref) {
        // collapsed reference link
        let seg: text::Segment = (
            ctx.link_labels().get_elem(last_ref)?.index(arena).stop(),
            start_pos.start() - 1,
        )
            .into();
        link_ref_value = reader.between(seg.into());
        maybe_link_ref = link_ref_value.bytes(reader.source());
        kind = LinkReferenceKind::Collapsed;
    }
    // CommonMark spec says:
    //  > A link label can have at most 999 characters inside the square brackets.
    if maybe_link_ref.len() > 999 {
        return None;
    }

    let maybe_link_ref = to_link_reference(maybe_link_ref);
    let link_ref =
        ctx.link_reference(unsafe { core::str::from_utf8_unchecked(&maybe_link_ref) })?;
    let ref_def = as_kind_data!(arena, link_ref, LinkReferenceDefinition);

    Some(match ref_def.title().as_option() {
        Some(t) => {
            Link::reference_with_title(ref_def.destination(), link_ref_value, kind, t.clone())
        }
        None => Link::reference(ref_def.destination(), link_ref_value, kind),
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
    // CommonMark disallows links inside link text. Propagate the descendant
    // marker so any still-open outer `[...]` opener turns into literal text
    // when its closer is reached. Images inside links are allowed.
    if !ctx.link_labels().data(last_ref).is_image() {
        ctx.set_has_link_descendant();
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
    let destination = destination.unwrap_or(text::Value::Empty);
    Some(match title {
        Some(t) => Link::inline_with_title(destination, t),
        None => Link::inline(destination),
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

    let title = reader.between_current(l, pos);
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
            } else if c == b'<' {
                // A pointy-bracket destination must not contain an unescaped '<'.
                return None;
            } else if c == b'>' {
                reader.advance(i + 1);
                return Some(text::Value::from_segment(
                    Segment::new(segment.start() + 1, segment.start() + i),
                    reader.decoder(),
                ));
            }
            i += 1;
        }
        return None;
    }
    let mut opened: isize = 0;
    let mut i = 0;

    while i < line.len() {
        let c = line[i];
        if c == b'\\' && i < line.len() - 1 && is_punct(line[i + 1]) {
            i += 2;
            continue;
        } else if c == b'(' {
            opened += 1;
            if opened > MAX_LINK_DESTINATION_NEST as isize {
                return None;
            }
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
    Some(text::Value::from_segment(
        Segment::new(segment.start(), segment.start() + i),
        reader.decoder(),
    ))
}

fn parse_simple_link(
    arena: &mut Arena,
    reader: &mut text::BlockReader,
    line: &[u8],
    segment: Segment,
    is_image: bool,
    ctx: &mut Context,
) -> Option<NodeRef> {
    let content_start = if is_image { 2 } else { 1 };
    if content_start >= line.len() || line.len() > SIMPLE_LINK_MAX {
        return None;
    }

    let triggerable = ctx.simple_link_triggerable();
    let mut close_bracket = None;
    let mut i = content_start;
    while i < line.len() {
        let ch = line[i];
        if ch == b'\\' && i + 1 < line.len() {
            // An escaped byte is plain content, not syntax.
            i += 2;
            continue;
        }
        if ch == b'\n' || ch == b'[' {
            return None;
        }
        if ch == b']' {
            close_bracket = Some(i);
            break;
        }
        if triggerable[ch as usize] {
            return None;
        }
        i += 1;
    }
    let close_bracket = close_bracket?;
    // The index of the label content, i.e. between `[` and `]`.
    let label_segment = Segment::new(
        segment.start() + content_start,
        segment.start() + close_bracket,
    );

    // Commit the reader past `[label]` and delegate the tail to the shared
    // helpers. Anything that fails there rewinds and lets the standard
    // `[...]` state machine take another swing.
    let (saved_line, saved_pos) = reader.position();
    reader.advance(close_bracket + 1);
    let link = match reader.peek_byte() {
        b'(' => parse_link(reader),
        b'[' => parse_reference_link_tail(arena, reader, label_segment, ctx),
        _ => parse_shortcut_link_tail(arena, reader, label_segment, ctx),
    };
    let Some(link) = link else {
        reader.set_position(saved_line, saved_pos);
        return None;
    };

    let label_text_ref = arena.new_node(Text::new(text::Value::from_segment(
        label_segment,
        reader.decoder(),
    )));
    let link_ref = if is_image {
        arena.new_node(Image::from_link(link))
    } else {
        arena.new_node(link)
    };
    arena[link_ref].set_pos(segment.start());
    link_ref.append_child_fast(arena, label_text_ref);

    if !is_image {
        // CommonMark disallows links inside link text; propagate the
        // descendant marker to any still-open outer link label.
        ctx.set_has_link_descendant();
    }
    Some(link_ref)
}

/// Parses the `[ref]` tail of a full or collapsed reference link, given the
/// index of the original link label (used for collapsed references).
fn parse_reference_link_tail(
    arena: &Arena,
    reader: &mut text::BlockReader,
    label_segment: Segment,
    ctx: &Context,
) -> Option<Link> {
    reader.advance(1); // skip '['
    let (l, pos) = reader.position();
    let mut closed = false;
    let mut opened = 0isize;
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
    let mut link_ref_value = reader.between_current(l, pos);
    reader.advance(1); // skip ']'

    let collapsed = {
        let maybe_ref = link_ref_value.bytes(reader.source());
        is_blank(&maybe_ref)
    };
    let (def, kind) = {
        let maybe_ref = if collapsed {
            // Collapsed reference link: the label is the original link label.
            link_ref_value = text::Value::from_segment(label_segment, reader.decoder());
            link_ref_value.bytes(reader.source())
        } else {
            link_ref_value.bytes(reader.source())
        };
        // CommonMark spec says:
        //  > A link label can have at most 999 characters inside the square brackets.
        if maybe_ref.len() > 999 {
            return None;
        }
        let maybe_ref = to_link_reference(maybe_ref);
        let def = ctx.link_reference(unsafe { core::str::from_utf8_unchecked(&maybe_ref) })?;
        let kind = if collapsed {
            LinkReferenceKind::Collapsed
        } else {
            LinkReferenceKind::Full
        };
        (def, kind)
    };
    let ref_def = as_kind_data!(arena, def, LinkReferenceDefinition);
    Some(match ref_def.title().as_option() {
        Some(t) => {
            Link::reference_with_title(ref_def.destination(), link_ref_value, kind, t.clone())
        }
        None => Link::reference(ref_def.destination(), link_ref_value, kind),
    })
}

/// Parses a shortcut reference link tail, given the index of the label.
fn parse_shortcut_link_tail(
    arena: &Arena,
    reader: &text::BlockReader,
    label_segment: Segment,
    ctx: &Context,
) -> Option<Link> {
    let ref_value = text::Value::from_segment(label_segment, reader.decoder());
    let def = {
        let maybe_ref = ref_value.bytes(reader.source());
        if maybe_ref.is_empty() || maybe_ref.len() > 999 {
            return None;
        }
        let maybe_ref = to_link_reference(maybe_ref);
        ctx.link_reference(unsafe { core::str::from_utf8_unchecked(&maybe_ref) })?
    };
    let ref_def = as_kind_data!(arena, def, LinkReferenceDefinition);
    Some(match ref_def.title().as_option() {
        Some(t) => Link::reference_with_title(
            ref_def.destination(),
            ref_value,
            LinkReferenceKind::Shortcut,
            t.clone(),
        ),
        None => Link::reference(
            ref_def.destination(),
            ref_value,
            LinkReferenceKind::Shortcut,
        ),
    })
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
    let node_ref = arena.new_node(Text::with_qualifiers(
        text::Value::from_segment(seg, reader.decoder()),
        TextQualifier::TEMP,
    ));
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
