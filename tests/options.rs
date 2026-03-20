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
