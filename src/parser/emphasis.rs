use crate::ast::{Arena, Emphasis, NodeRef};
use crate::parser::{parse_delimiter, Context, Delimiter, DelimiterProcessor, InlineParser};
use crate::text::{self};

/// [`InlineParser`] for emphasis.
#[derive(Debug)]
pub struct EmphasisParser {
    processor: DelimiterProcessor,
}

fn is_delimiter(c: u8) -> bool {
    c == b'*' || c == b'_'
}

fn can_open_closer(opener: &Delimiter, closer: &Delimiter) -> bool {
    opener.char() == closer.char()
}

fn on_match(arena: &mut Arena, consumes: usize) -> NodeRef {
    arena.new_node(Emphasis::new(consumes as u8))
}

impl EmphasisParser {
    /// Returns a new [`EmphasisParser`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for EmphasisParser {
    fn default() -> Self {
        let processor = DelimiterProcessor::new(is_delimiter, can_open_closer, on_match);
        Self { processor }
    }
}

impl InlineParser for EmphasisParser {
    fn trigger(&self) -> &[u8] {
        b"*_"
    }

    fn parse(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut Context,
    ) -> Option<NodeRef> {
        parse_delimiter(arena, parent_ref, reader, 1, &self.processor, ctx)
    }
}
