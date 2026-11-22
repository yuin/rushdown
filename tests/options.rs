use std::{fs, path::PathBuf};

use rushdown::{new_markdown_to_html, parser, renderer::html, test::MarkdownTestSuite};

fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn test_options() {
    let path = data_path("options.txt");
    let s = fs::read_to_string(&path).expect("failed to read options.txt");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options {
            auto_heading_ids: true,
            attributes: true,
            ..Default::default()
        },
        html::Options {
            allows_unsafe: true,
            xhtml: true,
            ..html::Options::default()
        },
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    suite.execute(&markdown_to_html)
}

#[test]
fn test_unsafe_url() {
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options::default(),
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    let mut output = String::new();
    {
        output.clear();
        let input = r#"
[click](&#106;avascript:alert(1))
"#;
        markdown_to_html(&mut output, input).expect("failed to render");
        assert_eq!(
            output,
            r#"<p><a href="">click</a></p>
"#
        );
    }
    {
        output.clear();
        let input = r#"
![alt](javascript:alert(document.domain))
"#;
        markdown_to_html(&mut output, input).expect("failed to render");
        assert_eq!(
            output,
            r#"<p><img src="" alt="alt"></p>
"#
        );
    }
    {
        output.clear();
        let input = r#"
<javascript:alert(document.domain)>
"#;
        markdown_to_html(&mut output, input).expect("failed to render");
        assert_eq!(
            output,
            r#"<p><a href="">javascript:alert(document.domain)</a></p>
"#
        );
    }
}

#[test]
fn test_parse_options() {
    let key = std::rc::Rc::new(std::cell::Cell::new(None));
    let k = key.clone();
    let p = parser::Parser::with_options(parser::Options {
        context_key_registry_initializer: Some(std::rc::Rc::new(
            move |reg: &mut rushdown::context::ContextKeyRegistry| {
                k.set(Some(
                    reg.get_or_create::<rushdown::context::BoolValue>("test.key"),
                ));
            },
        )),
        ..Default::default()
    });
    let key = key.get().unwrap();
    let mut called = false;
    let mut init = |ctx: &mut parser::Context, _: &rushdown::context::ContextKeyRegistry| {
        ctx.insert(key, true);
        assert_eq!(ctx.get(key), Some(&true));
        called = true;
    };
    let mut out = String::new();
    let opts = parser::ParseOptions {
        context_initializer: Some(&mut init),
        pretty_print: parser::PrettyPrintTarget::Writer(&mut out),
    };
    let (_arena, _doc) = p.parse("hello\n", opts);
    assert!(called);
    assert!(out.contains("Document"));
}

#[test]
fn test_render_options() {
    let key = std::rc::Rc::new(std::cell::Cell::new(None));
    let k = key.clone();
    let r = html::Renderer::<String>::with_options(html::Options {
        context_key_registry_initializer: Some(std::rc::Rc::new(
            move |reg: &mut rushdown::context::ContextKeyRegistry| {
                k.set(Some(
                    reg.get_or_create::<rushdown::context::BoolValue>("test.key"),
                ));
            },
        )),
        ..Default::default()
    });
    let key = key.get().unwrap();
    let mut called = false;
    let mut init = |ctx: &mut rushdown::renderer::Context,
                    _: &rushdown::context::ContextKeyRegistry| {
        ctx.insert(key, true);
        assert_eq!(ctx.get(key), Some(&true));
        called = true;
    };
    let (arena, doc) = parser::Parser::new().parse("hello\n", Default::default());
    let mut out = String::new();
    r.render(
        &mut out,
        "hello\n",
        &arena,
        doc,
        rushdown::renderer::RenderOptions {
            context_initializer: Some(&mut init),
        },
    )
    .unwrap();
    assert!(called);
    assert_eq!(out, "<p>hello</p>\n");
}

#[test]
fn test_value_write_to_decodes_and_encodes_in_one_pass() {
    use rushdown::{
        renderer::html,
        text::{Decoder, Index, Value},
    };
    let src = "a &lt;b&gt; \\* &amp;x &#35; \\ y &unknown; \0";
    for d in [Decoder::Default, Decoder::EscapedSpace, Decoder::Identity] {
        let v = Value::from_index(Index::new(0, src.len()), d);
        let mut one_pass = String::new();
        v.write_to(&mut html::TextWriter::new(&mut one_pass), src)
            .unwrap();
        let mut two_pass = String::new();
        html::write_text(&mut two_pass, &v.value(src)).unwrap();
        assert_eq!(one_pass, two_pass);
    }
}

#[test]
fn test_link_url_writer() {
    use rushdown::renderer::html;
    let mut out = String::new();
    html::write_link_url(&mut out, "/a b?x=1&y=\"é").unwrap();
    assert_eq!(out, "/a%20b?x=1&amp;y=%22%C3%A9");
}

#[test]
fn test_parse_delimiter_simple() {
    let path = data_path("parse-delimiter-simple.txt");
    let s = fs::read_to_string(&path).expect("failed to read parse-delimiter-simple.txt");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options {
            parse_delimiter: Some(parser::parse_delimiter_simple),
            ..Default::default()
        },
        html::Options {
            allows_unsafe: true,
            xhtml: true,
            ..html::Options::default()
        },
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    suite.execute(&markdown_to_html)
}

#[test]
fn test_line_break_strategy_simple_east_asian() {
    let path = data_path("line-break-strategy-simple.txt");
    let s = fs::read_to_string(&path).expect("failed to read line-break-strategy-simple.txt");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options {
            escaped_space: true,
            ..Default::default()
        },
        html::Options {
            allows_unsafe: true,
            xhtml: true,
            line_break_strategy: Some(html::LineBreakStrategy::SimpleEastAsian),
            ..html::Options::default()
        },
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    suite.execute(&markdown_to_html)
}

#[test]
fn test_line_break_strategy_css_text3() {
    let path = data_path("line-break-strategy-css-text3.txt");
    let s = fs::read_to_string(&path).expect("failed to read line-break-strategy-css-text3.txt");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            xhtml: true,
            line_break_strategy: Some(html::LineBreakStrategy::CssText3),
            ..html::Options::default()
        },
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    suite.execute(&markdown_to_html)
}

#[test]
fn test_hard_wraps_take_precedence_over_line_break_strategy() {
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            xhtml: true,
            hard_wraps: true,
            line_break_strategy: Some(html::LineBreakStrategy::SimpleEastAsian),
            ..html::Options::default()
        },
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    let mut out = String::new();
    markdown_to_html(&mut out, "言った\nんです").unwrap();
    assert_eq!(out, "<p>言った<br />\nんです</p>\n");
}

#[test]
fn test_custom_line_break_strategy() {
    fn never_break(_last: Option<char>, _first: Option<char>) -> bool {
        false
    }
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            line_break_strategy: Some(html::LineBreakStrategy::Custom(never_break)),
            ..html::Options::default()
        },
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    let mut out = String::new();
    markdown_to_html(&mut out, "a\nb").unwrap();
    assert_eq!(out, "<p>ab</p>\n");
}

#[derive(Debug)]
struct Mention;

impl rushdown::ast::NodeKind for Mention {
    fn typ(&self) -> rushdown::ast::NodeType {
        rushdown::ast::NodeType::Inline
    }
    fn kind_name(&self) -> &'static str {
        "Mention"
    }
}

impl rushdown::ast::InlineChars for Mention {
    fn first_char(
        &self,
        _node: &rushdown::ast::Node,
        _arena: &rushdown::ast::Arena,
        _source: &str,
    ) -> Option<char> {
        Some('@')
    }
}

impl rushdown::ast::PrettyPrint for Mention {
    fn pretty_print(
        &self,
        _w: &mut dyn core::fmt::Write,
        _s: &str,
        _l: usize,
    ) -> core::fmt::Result {
        Ok(())
    }
}

impl From<Mention> for rushdown::ast::KindData {
    fn from(e: Mention) -> Self {
        rushdown::ast::KindData::Extension(Box::new(e))
    }
}

#[test]
fn test_extension_first_and_last_char() {
    use rushdown::ast::{Arena, Text};
    let mut arena = Arena::new();
    let m = arena.new_node(Mention);
    let t = arena.new_node(Text::new("ab"));
    m.append_child(&mut arena, t);
    // overridden first_char, default last_char descends into children
    assert_eq!(arena[m].first_char(&arena, ""), Some('@'));
    assert_eq!(arena[m].last_char(&arena, ""), Some('b'));

    // block nodes do not have inline characters
    let p = arena.new_node(rushdown::ast::Paragraph::new());
    let t = arena.new_node(Text::new("ab"));
    p.append_child(&mut arena, t);
    assert_eq!(arena[p].first_char(&arena, ""), None);
    assert_eq!(arena[p].last_char(&arena, ""), None);
}
