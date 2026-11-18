use crate::{
    ast::{Arena, Emphasis, NodeRef, Strong},
    parser::{
        parse_delimiter, Context, Delimiter, DelimiterProcessor, InlineParser, Options,
        ParseDelimiterFn,
    },
    text::{self},
};

/// [`InlineParser`] for emphasis.
#[derive(Debug)]
pub struct EmphasisParser {
    processor: DelimiterProcessor,
    parse_delimiter: ParseDelimiterFn,
}

fn is_delimiter(c: u8) -> bool {
    c == b'*' || c == b'_'
}

fn can_open_closer(opener: &Delimiter, closer: &Delimiter) -> bool {
    opener.char() == closer.char()
}

fn on_match(arena: &mut Arena, consumes: usize) -> NodeRef {
    if consumes == 2 {
        return arena.new_node(Strong::new());
    }
    arena.new_node(Emphasis::new())
}

impl EmphasisParser {
    /// Returns a new [`EmphasisParser`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a new [`EmphasisParser`] with the given parser options.
    pub fn with_options(options: Options) -> Self {
        Self {
            parse_delimiter: options.parse_delimiter.unwrap_or(parse_delimiter),
            ..Self::default()
        }
    }
}

impl Default for EmphasisParser {
    fn default() -> Self {
        let processor = DelimiterProcessor::new(is_delimiter, can_open_closer, on_match);
        Self {
            processor,
            parse_delimiter,
        }
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
        (self.parse_delimiter)(arena, parent_ref, reader, 1, &self.processor, ctx)
    }
}
