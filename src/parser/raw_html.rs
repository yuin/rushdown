use crate::ast::{Arena, NodeRef, RawHtml};
use crate::parser::{Context, InlineParser};
use crate::scanner::{
    scan_html_cdata_reader, scan_html_comment_reader, scan_html_declaration_reader,
    scan_html_processing_instruction_reader, scan_html_tag_reader,
};
use crate::text::{self, Reader};

/// [`InlineParser`] for inline raw HTML.
#[derive(Debug, Default)]
pub struct RawHtmlParser {}

impl RawHtmlParser {
    /// Returns a new [`RawHtmlParser`].
    pub fn new() -> Self {
        Self::default()
    }
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
        _ctx: &mut Context,
    ) -> Option<NodeRef> {
        let (line, pos) = reader.position();
        if scan_html_tag_reader(reader).is_some()
            || scan_html_comment_reader(reader).is_some()
            || scan_html_processing_instruction_reader(reader).is_some()
            || scan_html_declaration_reader(reader).is_some()
            || scan_html_cdata_reader(reader).is_some()
        {
            return Some(arena.new_node(RawHtml::new(
                reader.between_current(line, pos).into_iter().collect(),
            )));
        }
        None
    }
}
