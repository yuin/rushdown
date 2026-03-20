extern crate alloc;

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;
use crate::{
    ast::{Arena, NodeRef, Text, TextQualifier},
    parser::{Context, DelimiterProcessor, ParseStackElemRef},
    text::Segment,
};
use crate::{
    parser::Delimiter,
    text::{self, Reader},
    util::{char_at, is_unicode_space, is_unicode_symbol_or_punct},
};

/// Parse a delimiter run in the reader.
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
    let mut j = 0;
    if segment.padding() != 0 || !processor.is_delimiter(c) {
        return None;
    }
    while j < line.len() && c == line[j] {
        j += 1;
    }
    if j >= minimum {
        let precending = reader.precending_charater();
        let after = char_at(&line, j).unwrap_or(' ');
        let before_is_punct = is_unicode_symbol_or_punct(precending);
        let before_is_space = is_unicode_space(precending);
        let after_is_punct = is_unicode_symbol_or_punct(after);
        let after_is_space = is_unicode_space(after);

        let is_left = !after_is_space && (!after_is_punct || before_is_space || before_is_punct);
        let is_right = !before_is_space && (!before_is_punct || after_is_space || after_is_punct);
        let can_open = if c == b'_' {
            is_left && (!is_right || before_is_punct)
        } else {
            is_left
        };
        let can_close = if c == b'_' {
            is_right && (!is_left || after_is_punct)
        } else {
            is_right
        };
        reader.advance(j);
        let delim = Delimiter::new(c, j, can_open, can_close, processor.clone());
        let seg: Segment = (segment.start(), segment.start() + j).into();
        let t = arena.new_node(Text::with_qualifiers(seg, TextQualifier::TEMP));

        ctx.delimiters_mut().push(delim, t);
        return Some(t);
    }
    None
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
            opener_ref_opt = closer.previous();
            while let Some(opener_ref) = opener_ref_opt {
                let opener = ctx.delimiters().elem(opener_ref);
                if let Some(bottom) = bottom {
                    if opener_ref == bottom {
                        break;
                    }
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
        #[cfg(feature = "inline-pos")]
        {
            use crate::ast::Pos;
            use crate::{as_type_data, as_type_data_mut};
            let opener = ctx.delimiters().elem(opener_ref).node_ref();
            let pos = as_type_data!(arena, opener, Inline).pos();
            as_type_data_mut!(arena, node_ref, Inline).set_pos(pos);
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
