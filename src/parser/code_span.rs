use crate::ast::{Arena, CodeSpan, NodeRef, Text};
use crate::parser::{Context, InlineParser};
use crate::text::{self, Reader, Segment};
use crate::util::TinyVec;

/// [`InlineParser`] for inline codes.
#[derive(Debug, Default)]
pub struct CodeSpanParser {}

impl CodeSpanParser {
    /// Returns a new [`CodeSpanParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl InlineParser for CodeSpanParser {
    fn trigger(&self) -> &[u8] {
        b"`"
    }

    fn parse(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        _ctx: &mut Context,
    ) -> Option<NodeRef> {
        let (line, start_segment) = reader.peek_line_bytes()?;
        let opener = line.iter().take_while(|&&c| c == b'`').count();
        reader.advance(opener);
        let (l, pos) = reader.position();
        let mut lines = TinyVec::<text::Index>::empty();
        let mut blank = true;
        'try_lines: loop {
            let Some(line_segment_opt) = reader.peek_line_bytes() else {
                reader.set_position(l, pos);
                return Some(arena.new_node(Text::new(Segment::new(
                    start_segment.start(),
                    start_segment.start() + opener,
                ))));
            };
            let (line, mut segment) = line_segment_opt;
            let mut i = 0;
            while i < line.len() {
                let c = line[i];
                if c == b'`' {
                    let old_i = i;
                    while i < line.len() && line[i] == b'`' {
                        i += 1;
                    }
                    let closure = i - old_i;
                    if closure == opener && (i >= line.len() || line[i] != b'`') {
                        segment = segment.with_stop(segment.start() + i - closure);
                        if !segment.is_empty() {
                            lines.push(segment.into());
                            if !segment.is_blank(reader.source()) {
                                blank = false;
                            }
                        }
                        reader.advance(i);
                        break 'try_lines;
                    }
                } else {
                    i += 1;
                }
            }
            if i < line.len() {
                break;
            }
            if !segment.is_blank(reader.source()) {
                blank = false;
            }
            lines.push(segment.into());
            reader.advance_line();
        }

        if !blank {
            // trim first halfspace and last halfspace
            if let Some(fidx) = lines.first() {
                let bsource = reader.source().as_bytes();
                if fidx.is_empty() || !is_space_or_newline(bsource[fidx.start()]) {
                    return Some(arena.new_node(CodeSpan::from_indices(lines)));
                }
                {
                    let lidx = lines.last().unwrap();
                    if lidx.is_empty() || !is_space_or_newline(bsource[lidx.stop() - 1]) {
                        return Some(arena.new_node(CodeSpan::from_indices(lines)));
                    }
                }

                lines[0] = (fidx.start() + 1, fidx.stop()).into();
                let l = lines.len() - 1;
                lines[l] = (lines[l].start(), lines[l].stop() - 1).into();
            }
        }
        Some(arena.new_node(CodeSpan::from_indices(lines)))
    }
}

fn is_space_or_newline(c: u8) -> bool {
    c == b' ' || c == b'\n'
}
