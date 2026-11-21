use std::time::Instant;

use rushdown::{
    ast::{Arena, Emphasis, NodeRef},
    new_markdown_to_html_string, parser,
    parser::AnyInlineParser,
    renderer::html,
    text,
};

#[derive(Debug)]
struct EqualsDelimiterParser {
    processor: parser::DelimiterProcessor,
}

fn is_delimiter(c: u8) -> bool {
    c == b'='
}

fn can_open_closer(opener: &parser::Delimiter, closer: &parser::Delimiter) -> bool {
    opener.char() == closer.char()
}

fn on_match(arena: &mut Arena, _consumes: usize) -> NodeRef {
    arena.new_node(Emphasis::new())
}

impl EqualsDelimiterParser {
    fn new() -> Self {
        Self {
            processor: parser::DelimiterProcessor::new(is_delimiter, can_open_closer, on_match),
        }
    }
}

impl From<EqualsDelimiterParser> for AnyInlineParser {
    fn from(p: EqualsDelimiterParser) -> Self {
        AnyInlineParser::Extension(Box::new(p))
    }
}

impl parser::InlineParser for EqualsDelimiterParser {
    fn trigger(&self) -> &[u8] {
        b"="
    }

    fn parse(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut parser::Context,
    ) -> Option<NodeRef> {
        parser::parse_delimiter(arena, parent_ref, reader, 1, &self.processor, ctx)
    }
}

fn markdown_to_html() -> impl Fn(&mut String, &str) -> rushdown::Result<()> {
    new_markdown_to_html_string(
        parser::Options::default(),
        html::Options::default(),
        parser::parser_extension(|p| {
            p.add_inline_parser(
                EqualsDelimiterParser::new,
                parser::NoParserOptions,
                parser::PRIORITY_EMPHASIS + 100,
            );
        }),
        html::NO_EXTENSIONS,
    )
}

#[test]
fn test_arbitrary_delimiter() {
    let to_html = markdown_to_html();
    let mut out = String::new();
    to_html(&mut out, "=mark=").unwrap();
    assert_eq!(out, "<p><em>mark</em></p>\n");
}

/// The `=` closers must take part in the openers-bottom cache. Without an entry
/// for non-builtin delimiter characters, each closer rescans all preceding `*`
/// openers and this input becomes quadratic.
#[test]
#[cfg(not(debug_assertions))]
fn test_arbitrary_delimiter_unmatched_performance() {
    let n = 40_000;
    let source = format!("{}{}", "*a ".repeat(n), "a= ".repeat(n));
    let to_html = markdown_to_html();
    let mut out = String::new();
    let started = Instant::now();
    to_html(&mut out, &source).unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "parsing unmatched arbitrary delimiters took too long: {:?}",
        started.elapsed()
    );
}
