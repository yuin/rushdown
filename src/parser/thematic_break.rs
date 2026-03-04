use crate::ast::{Arena, NodeRef, ThematicBreak};
use crate::parser::{BlockParser, Context, State};
use crate::text;
use crate::text::Reader as _;
use crate::util::{indent_width, is_space};

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
        _ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        let (line, _) = reader.peek_line_bytes()?;

        if is_thematic_break(line.as_ref(), reader.line_offset()) {
            reader.advance_to_eol();
            Some((arena.new_node(ThematicBreak::default()), State::NO_CHILDREN))
        } else {
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

pub(super) fn is_thematic_break(line: &[u8], offset: usize) -> bool {
    let (w, pos) = indent_width(line, offset);
    if w > 3 {
        return false;
    }
    let mut mark: u8 = 0;
    let mut count = 0;
    for &c in &line[pos..] {
        if is_space(c) {
            continue;
        }
        if mark == 0 {
            mark = c;
            count = 1;
            if mark == b'*' || mark == b'-' || mark == b'_' {
                continue;
            }
            return false;
        }
        if c != mark {
            return false;
        }
        count += 1;
    }
    count > 2
}
