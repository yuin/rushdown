use memchr::memmem;

use crate::{
    ast::{Arena, NodeRef, RawHtml},
    parser::{Context, InlineParser},
    scanner::scan_html_tag_reader,
    text::{self, Reader},
};

/// [`InlineParser`] for inline raw HTML.
#[derive(Debug, Default)]
pub struct RawHtmlParser {}

impl RawHtmlParser {
    /// Returns a new [`RawHtmlParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

/// Scans for `closer` from the current position, up to the end of the reader.
///
/// `start_offset` is the number of bytes to skip on the first line. The
/// `cache` stores the end of the last line that was searched and did not
/// contain `closer`. Any later search that starts before that position cannot
/// possibly find `closer`, so it is skipped. This keeps inputs with many
/// unclosed raw HTML fragments linear in the input size.
fn scan_until<'a>(
    reader: &mut text::BlockReader<'a>,
    closer: &[u8],
    start_offset: usize,
    cache: &mut usize,
) -> Option<()> {
    let (saved_line, saved_pos) = reader.position();
    let mut last_stop = 0;
    let mut offset = start_offset;
    while let Some((line, segment)) = reader.peek_line_bytes() {
        if segment.start() < *cache {
            break;
        }
        if offset > 0 {
            if offset < line.len() {
                if let Some(pos) = memmem::find(&line[offset..], closer) {
                    reader.advance(offset + pos + closer.len());
                    return Some(());
                }
            }
        } else if let Some(pos) = memmem::find(&line, closer) {
            reader.advance(pos + closer.len());
            return Some(());
        }
        offset = 0;
        last_stop = segment.stop();
        reader.advance_line();
    }
    *cache = (*cache).max(last_stop);
    reader.set_position(saved_line, saved_pos);
    None
}

impl InlineParser for RawHtmlParser {
    fn trigger(&self) -> &[u8] {
        b"<"
    }

    fn parse(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut Context,
    ) -> Option<NodeRef> {
        let (line, pos) = reader.position();
        let (line_bytes, _) = reader.peek_line_bytes()?;

        if scan_html_tag_reader(reader).is_some() {
            return Some(
                arena.new_node(RawHtml::new(
                    reader
                        .between_current(line, pos)
                        .with_decoder(text::Decoder::Identity),
                )),
            );
        }

        let mut found = false;
        if line_bytes.starts_with(b"<!-->") {
            reader.advance(5);
            found = true;
        } else if line_bytes.starts_with(b"<!--->") {
            reader.advance(6);
            found = true;
        } else if line_bytes.starts_with(b"<!--") {
            found = scan_until(reader, b"-->", 4, &mut ctx.raw_html_comment_cache).is_some();
        }
        if !found {
            if line_bytes.starts_with(b"<?") {
                found = scan_until(
                    reader,
                    b"?>",
                    0,
                    &mut ctx.raw_html_processing_instruction_cache,
                )
                .is_some();
            } else if line_bytes.len() > 2
                && line_bytes[1] == b'!'
                && line_bytes[2].is_ascii_alphabetic()
            {
                found = scan_until(reader, b">", 0, &mut ctx.raw_html_declaration_cache).is_some();
            } else if line_bytes.starts_with(b"<![CDATA[") {
                found = scan_until(reader, b"]]>", 0, &mut ctx.raw_html_cdata_cache).is_some();
            }
        }

        if found {
            Some(
                arena.new_node(RawHtml::new(
                    reader
                        .between_current(line, pos)
                        .with_decoder(text::Decoder::Identity),
                )),
            )
        } else {
            None
        }
    }
}
