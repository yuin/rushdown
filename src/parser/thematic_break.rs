use crate::{
    ast::{Arena, NodeRef, ThematicBreak},
    parser::{BlockParser, Context, State},
    text,
    text::Reader as _,
    util::{indent_width, is_space},
};

/// [`BlockParser`] for thematic breaks.
#[derive(Debug, Default)]
pub struct ThematicBreakParser {}

impl ThematicBreakParser {
    /// Returns a new [`ThematicBreakParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl BlockParser for ThematicBreakParser {
    fn trigger(&self) -> &[u8] {
        b"-*_"
    }

    fn open(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        let (line, segment) = reader.peek_line_bytes()?;

        // A previous scan of this same line failed at `brk`. Nested parsers
        // can retry the same line many times, so short-circuit those retries.
        let cache = ctx.thematic_break_cache;
        if cache.stop == segment.stop() {
            if let Some(brk) = cache.brk {
                for (i, &c) in line.iter().enumerate() {
                    if c == b' ' || c == b'\t' {
                        continue;
                    }
                    if c == cache.mark && segment.start() + i < brk {
                        return None;
                    }
                    break;
                }
            }
        }

        let (ok, mark, _, brk) = scan_thematic_break(line.as_ref(), reader.line_offset());
        if ok {
            reader.advance_to_eol();
            Some((arena.new_node(ThematicBreak::default()), State::NO_CHILDREN))
        } else {
            if let Some(brk) = brk {
                ctx.thematic_break_cache.stop = segment.stop();
                ctx.thematic_break_cache.mark = mark;
                ctx.thematic_break_cache.brk = Some(segment.start() + brk);
            }
            None
        }
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

    fn can_interrupt_paragraph(&self) -> bool {
        true
    }
}

pub(super) fn scan_thematic_break(line: &[u8], offset: usize) -> (bool, u8, usize, Option<usize>) {
    let (w, pos) = indent_width(line, offset);
    if w > 3 {
        return (false, 0, pos, None);
    }
    let mut mark: u8 = 0;
    let mut count = 0;
    for (i, &c) in line[pos..].iter().enumerate() {
        if is_space(c) {
            continue;
        }
        if mark == 0 {
            mark = c;
            count = 1;
            if mark == b'*' || mark == b'-' || mark == b'_' {
                continue;
            }
            return (false, mark, pos, None);
        }
        if c != mark {
            return (false, mark, pos, Some(pos + i));
        }
        count += 1;
    }
    (count > 2, mark, pos, None)
}

pub(super) fn is_thematic_break(line: &[u8], offset: usize) -> bool {
    scan_thematic_break(line, offset).0
}
