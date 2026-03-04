use crate::as_kind_data_mut;
use crate::ast::{Arena, CodeSpan, NodeRef, Text, TextQualifier};
use crate::parser::{Context, InlineParser};
use crate::text::{self, Reader, Segment};

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
        let code_span_ref = arena.new_node(CodeSpan::new());
        let mut blank = true;
        'try_lines: loop {
            let Some(line_segment_opt) = reader.peek_line_bytes() else {
                reader.set_position(l, pos);
                code_span_ref.delete(arena);
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
                            let text_node_ref =
                                arena.new_node(Text::with_qualifiers(segment, TextQualifier::RAW));
                            code_span_ref.append_child_fast(arena, text_node_ref);
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
            let text_node_ref = arena.new_node(Text::with_qualifiers(segment, TextQualifier::RAW));
            code_span_ref.append_child_fast(arena, text_node_ref);
            reader.advance_line();
        }

        if !blank {
            // trim first halfspace and last halfspace
            if let Some(first_child_ref) = arena[code_span_ref].first_child() {
                let bsource = reader.source().as_bytes();
                {
                    let ftext = as_kind_data_mut!(arena, first_child_ref, Text);
                    let fseg_ref = ftext.segment().unwrap();
                    if fseg_ref.is_empty() || !is_space_or_newline(bsource[fseg_ref.start()]) {
                        return Some(code_span_ref);
                    }
                }
                {
                    let last_child_ref = arena[code_span_ref].last_child().unwrap();
                    let ltext = as_kind_data_mut!(arena, last_child_ref, Text);
                    let lseg_ref = ltext.segment().unwrap();
                    if lseg_ref.is_empty() || !is_space_or_newline(bsource[lseg_ref.stop() - 1]) {
                        return Some(code_span_ref);
                    }
                }

                let ftext = as_kind_data_mut!(arena, first_child_ref, Text);
                let fseg = ftext
                    .segment()
                    .unwrap()
                    .with_start(ftext.segment().unwrap().start() + 1);
                ftext.set(fseg);

                let last_child_ref = arena[code_span_ref].last_child().unwrap();
                let ltext = as_kind_data_mut!(arena, last_child_ref, Text);
                let lseg = ltext
                    .segment()
                    .unwrap()
                    .with_stop(ltext.segment().unwrap().stop() - 1);
                ltext.set(lseg);
            }
        }
        Some(code_span_ref)
    }
}

fn is_space_or_newline(c: u8) -> bool {
    c == b' ' || c == b'\n'
}
