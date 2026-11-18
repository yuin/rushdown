extern crate alloc;

use crate::{
    as_kind_data,
    ast::{Arena, NodeRef, Strikethrough},
    parser::{
        self, parse_delimiter, Delimiter, DelimiterProcessor, InlineParser, Options,
        ParseDelimiterFn,
    },
    text::{self, Reader},
};

/// [`InlineParser`] for strikethrough text.
#[derive(Debug)]
pub struct StrikethroughParser {
    processor: DelimiterProcessor,
    parse_delimiter: ParseDelimiterFn,
}

fn is_delimiter(c: u8) -> bool {
    c == b'~'
}

fn can_open_closer(opener: &Delimiter, closer: &Delimiter) -> bool {
    opener.char() == closer.char()
}

fn on_match(arena: &mut Arena, _consumes: usize) -> NodeRef {
    arena.new_node(Strikethrough::new())
}

impl Default for StrikethroughParser {
    fn default() -> Self {
        let processor = DelimiterProcessor::new(is_delimiter, can_open_closer, on_match);
        Self {
            processor,
            parse_delimiter,
        }
    }
}

impl StrikethroughParser {
    /// Returns a new [`StrikethroughParser`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a new [`StrikethroughParser`] with the given parser options.
    pub fn with_options(options: Options) -> Self {
        Self {
            parse_delimiter: options.parse_delimiter.unwrap_or(parse_delimiter),
            ..Self::default()
        }
    }
}

impl InlineParser for StrikethroughParser {
    fn trigger(&self) -> &[u8] {
        b"~"
    }

    fn parse(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut parser::Context,
    ) -> Option<NodeRef> {
        let precending = reader.precending_charater();
        let text = (self.parse_delimiter)(arena, parent_ref, reader, 1, &self.processor, ctx)?;
        let index = as_kind_data!(arena, text, Text).index()?;
        if index.len() > 2 || precending == '~' {
            return None;
        }
        Some(text)
    }
}
