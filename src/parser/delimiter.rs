extern crate alloc;

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;
use crate::{
    ast::{Arena, NodeRef, Text, TextQualifier},
    parser::{Context, Delimiter, DelimiterProcessor, ParseStackElemRef},
    text::{self, Reader, Segment},
    util::{char_at, is_punct, is_space, is_unicode_space, is_unicode_symbol_or_punct, HashMap},
};

/// Signature of functions that parse a delimiter run.
/// See [`parse_delimiter`] and [`parse_delimiter_simple`].
pub type ParseDelimiterFn = fn(
    &mut Arena,
    NodeRef,
    &mut text::BlockReader<'_>,
    usize,
    &DelimiterProcessor,
    &mut Context,
) -> Option<NodeRef>;

#[inline(always)]
fn char_class(c: char) -> (bool, bool) {
    if c.is_ascii() {
        let b = c as u8;
        return (is_space(b), is_punct(b));
    }
    (is_unicode_space(c), is_unicode_symbol_or_punct(c))
}

#[allow(clippy::too_many_arguments)]
fn push_delimiter(
    arena: &mut Arena,
    reader: &mut text::BlockReader<'_>,
    segment: &Segment,
    c: u8,
    j: usize,
    can_open: bool,
    can_close: bool,
    processor: &DelimiterProcessor,
    ctx: &mut Context,
) -> NodeRef {
    reader.advance(j);
    let delim = Delimiter::new(c, j, can_open, can_close, processor.clone());
    let seg: Segment = (segment.start(), segment.start() + j).into();
    let t = arena.new_node(Text::with_qualifiers(
        text::Value::from_segment(seg, reader.decoder()),
        TextQualifier::TEMP,
    ));
    ctx.delimiters_mut().push(delim, t);
    t
}

/// Parse a delimiter run in the reader that follows the CommonMark spec.
/// If a delimiter run is found, it is added to the context's delimiter list,
/// and the reader is advanced past the delimiter run.
/// Returns a [`NodeRef`] to the text node representing the delimiter run.
pub fn parse_delimiter<'a>(
    arena: &mut Arena,
    _parent_ref: NodeRef,
    reader: &mut text::BlockReader<'a>,
    minimum: usize,
    processor: &DelimiterProcessor,
    ctx: &mut Context,
) -> Option<NodeRef> {
    let (line, segment) = reader.peek_line_bytes()?;

    let c = line[0];
    if segment.padding() != 0 || !processor.is_delimiter(c) {
        return None;
    }
    let mut j = 0;
    while j < line.len() && c == line[j] {
        j += 1;
    }
    if j < minimum {
        return None;
    }
    let before = reader.precending_charater();
    let after = char_at(&line, j).unwrap_or(' ');
    let (before_is_space, before_is_punct) = char_class(before);
    let (after_is_space, after_is_punct) = char_class(after);

    let is_left = !after_is_space && (!after_is_punct || before_is_space || before_is_punct);
    let is_right = !before_is_space && (!before_is_punct || after_is_space || after_is_punct);
    let (can_open, can_close) = if c == b'_' {
        (
            is_left && (!is_right || before_is_punct),
            is_right && (!is_left || after_is_punct),
        )
    } else {
        (is_left, is_right)
    };
    Some(push_delimiter(
        arena, reader, &segment, c, j, can_open, can_close, processor, ctx,
    ))
}

/// A simpler implementation of [`ParseDelimiterFn`].
///
/// This function determines flankings based on original Markdown like simple rules:
///
/// - If the character after the delimiter is a space, the delimiter cannot open a span.
/// - If the character before the delimiter is a space, the delimiter cannot close a span.
///
/// These rules are easy to understand even for non-engineer writers.
/// While CommonMark rules do not work well with CJK, these rules often work well with CJK.
pub fn parse_delimiter_simple<'a>(
    arena: &mut Arena,
    _parent_ref: NodeRef,
    reader: &mut text::BlockReader<'a>,
    minimum: usize,
    processor: &DelimiterProcessor,
    ctx: &mut Context,
) -> Option<NodeRef> {
    let (line, segment) = reader.peek_line_bytes()?;

    let c = line[0];
    if segment.padding() != 0 || !processor.is_delimiter(c) {
        return None;
    }
    let mut j = 0;
    while j < line.len() && c == line[j] {
        j += 1;
    }
    if j < minimum {
        return None;
    }
    let before = reader.precending_charater();
    let after = char_at(&line, j).unwrap_or(' ');

    let mut before_is_delimiter = false;
    if let Some(last) = ctx.last_delimiter() {
        if ctx.delimiters().elem(last).index(arena).stop() == segment.start() {
            before_is_delimiter = true;
            ctx.delimiters_mut().data_mut(last).can_close = true;
        }
    }
    let is_left = !is_unicode_space(after);
    let is_right = !is_unicode_space(before) || before_is_delimiter;
    let (can_open, can_close) = if c == b'_' {
        (
            is_left && (!is_right || before_is_delimiter),
            is_right && !is_left,
        )
    } else {
        (is_left, is_right)
    };
    Some(push_delimiter(
        arena, reader, &segment, c, j, can_open, can_close, processor, ctx,
    ))
}

const DELIMITER_CLASS_COUNT: usize = 3;

/// Returns an index that groups closers by whether they can also open and by
/// their original length modulo 3.
#[inline(always)]
fn delimiter_class_index(can_open: bool, original_length_mod3: usize) -> usize {
    if can_open {
        original_length_mod3
    } else {
        DELIMITER_CLASS_COUNT + original_length_mod3
    }
}

/// The lowest opener position that may still match a closer, per delimiter
/// character and class.
///
/// Once a closer fails to find an opener, no later closer of the same character
/// and class can match an opener before it. Remembering that position keeps
/// long runs of unmatched delimiters from scanning the same openers over and
/// over, which would otherwise take quadratic time.
#[derive(Debug, Default)]
struct OpenersBottom {
    asterisk: [usize; DELIMITER_CLASS_COUNT * 2],
    underscore: [usize; DELIMITER_CLASS_COUNT * 2],
    tilde: [usize; DELIMITER_CLASS_COUNT * 2],
    /// Delimiters registered by extensions. This stays empty (and does not
    /// allocate) unless a non-builtin delimiter is actually used.
    other: HashMap<u8, [usize; DELIMITER_CLASS_COUNT * 2]>,
}

impl OpenersBottom {
    #[inline(always)]
    fn get(&self, c: u8, index: usize) -> usize {
        match c {
            b'*' => self.asterisk[index],
            b'_' => self.underscore[index],
            b'~' => self.tilde[index],
            _ => self.other.get(&c).map_or(0, |bounds| bounds[index]),
        }
    }

    #[inline(always)]
    fn set(&mut self, c: u8, index: usize, lower_bound: usize) {
        match c {
            b'*' => self.asterisk[index] = lower_bound,
            b'_' => self.underscore[index] = lower_bound,
            b'~' => self.tilde[index] = lower_bound,
            _ => {
                self.other
                    .entry(c)
                    .or_insert([0; DELIMITER_CLASS_COUNT * 2])[index] = lower_bound
            }
        }
    }
}

/// Processes the delimiter list in the context.
/// Processing will be stop when reaching the bottom.
///
/// If you implement an inline parser that can have other inline nodes as
/// children, you should call this function when nesting span has closed.
pub fn process_delimiters(arena: &mut Arena, bottom: Option<ParseStackElemRef>, ctx: &mut Context) {
    let Some(last_delimiter_ref) = ctx.last_delimiter() else {
        return;
    };
    let last_delimiter = ctx.delimiters().elem(last_delimiter_ref);
    let mut closer_ref_opt: Option<ParseStackElemRef> = None;
    if let Some(bottom_ref) = bottom {
        if bottom_ref != last_delimiter_ref {
            let mut c = last_delimiter.previous();
            while c.is_some_and(|c| c != bottom_ref) {
                closer_ref_opt = c;
                c = ctx
                    .delimiters()
                    .get_elem(c.unwrap())
                    .and_then(|d| d.previous());
            }
        }
    } else {
        closer_ref_opt = ctx.delimiters().bottom();
    }
    if closer_ref_opt.is_none() {
        ctx.delimiters_mut().remove_until(arena, bottom);
        return;
    };
    let mut openers_bottom = OpenersBottom::default();
    while let Some(closer_ref) = closer_ref_opt {
        let mut consume = 0usize;
        let mut found = false;
        let mut maybe_opener = false;
        let mut opener_ref_opt: Option<ParseStackElemRef>;

        {
            let closer = ctx.delimiters().elem(closer_ref);
            let closer_data = ctx.delimiters().data(closer_ref);
            if !closer_data.can_close() {
                closer_ref_opt = closer.next();
                continue;
            }
            let closer_char = closer_data.char();
            let closer_start = closer.index(arena).start();
            let class_index =
                delimiter_class_index(closer_data.can_open(), closer_data.length() % 3);
            let lower_bound = openers_bottom.get(closer_char, class_index);
            opener_ref_opt = closer.previous();
            while let Some(opener_ref) = opener_ref_opt {
                let opener = ctx.delimiters().elem(opener_ref);
                if let Some(bottom) = bottom {
                    if opener_ref == bottom {
                        break;
                    }
                }
                if lower_bound != 0 && opener.index(arena).start() < lower_bound - 1 {
                    break;
                }
                let opener_data = ctx.delimiters().data(opener_ref);
                if opener_data.can_open()
                    && opener_data
                        .processor()
                        .can_open_closer(opener_data, closer_data)
                {
                    maybe_opener = true;
                    consume = opener_data.calc_consumption(closer_data);
                    if consume > 0 {
                        found = true;
                        break;
                    }
                }
                opener_ref_opt = opener.previous();
            }
            if !found {
                openers_bottom.set(closer_char, class_index, closer_start + 1);
            }
        }
        if !found {
            let can_open = ctx.delimiters().data(closer_ref).can_open();
            let next = ctx.delimiters().elem(closer_ref).next();
            if !maybe_opener && !can_open {
                ctx.delimiters_mut().remove(arena, closer_ref);
            }
            closer_ref_opt = next;
            continue;
        }
        let opener_ref = opener_ref_opt.unwrap();

        ctx.delimiters_mut().data_mut(opener_ref).consume(consume);
        ctx.delimiters_mut().data_mut(closer_ref).consume(consume);

        let node_ref = (ctx.delimiters().data(opener_ref).processor().on_match)(arena, consume);
        let opener = ctx.delimiters().elem(opener_ref).node_ref();
        if let Some(p) = arena[opener].pos() {
            arena[node_ref].set_pos(p);
        }

        let o = ctx.delimiters().elem(opener_ref).node_ref();
        let p_opt = arena[o].parent();
        let mut c_opt = arena[o].next_sibling();
        let end = ctx.delimiters().elem(closer_ref).node_ref;
        while let Some(c) = c_opt {
            if c == end {
                break;
            }
            let next = arena[c].next_sibling();
            node_ref.append_child(arena, c);
            c_opt = next;
        }
        if let Some(p) = p_opt {
            p.insert_after(arena, o, node_ref);
        }

        let mut d_opt = ctx.delimiters().elem(opener_ref).next();
        while let Some(d_ref) = d_opt {
            if d_ref == closer_ref {
                break;
            }
            let next = ctx.delimiters().elem(d_ref).next();
            ctx.delimiters_mut().remove(arena, d_ref);
            d_opt = next;
        }

        if ctx.delimiters().data(opener_ref).remaining() == 0 {
            ctx.delimiters_mut().remove(arena, opener_ref);
        }

        if ctx.delimiters().data(closer_ref).remaining() == 0 {
            let next = ctx.delimiters().elem(closer_ref).next();
            ctx.delimiters_mut().remove(arena, closer_ref);
            closer_ref_opt = next;
        }
    }
    ctx.delimiters_mut().remove_until(arena, bottom);
}
