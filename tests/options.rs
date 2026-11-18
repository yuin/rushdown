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
