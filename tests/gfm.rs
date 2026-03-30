use std::{fs, path::PathBuf};

use rushdown::{
    as_kind_data,
    ast::Task,
    new_markdown_to_html,
    parser::{self, gfm, GfmOptions, LinkifyOptions},
    renderer::html,
    test::MarkdownTestSuite,
    Result,
};

fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn test_linkify() {
    let path = data_path("linkify.txt");
    let s = fs::read_to_string(&path).expect("failed to read spec.json");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            allows_unsafe: true,
            xhtml: true,
            ..html::Options::default()
        },
        gfm(GfmOptions::default()),
        html::NO_EXTENSIONS,
    );
    suite.execute(&markdown_to_html)
}

#[test]
fn test_linkify_scanner() {
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            allows_unsafe: true,
            xhtml: true,
            ..html::Options::default()
        },
        gfm(GfmOptions {
            linkify: LinkifyOptions {
                allowed_protocols: vec![
                    "http".to_string(),
                    "https".to_string(),
                    "ftp".to_string(),
                    "mailto".to_string(),
                    "custom".to_string(),
                ],
                url_scanner: Box::new(|b: &[u8]| -> Option<usize> {
                    let count = b.iter().take_while(|&&c| c != b' ').count();
                    if count > 0 && b.starts_with(b"custom://") {
                        Some(count)
                    } else {
                        None
                    }
                }),
                ..LinkifyOptions::default()
            },
        }),
        html::NO_EXTENSIONS,
    );
    let source =
        "Check out custom://example.com for more info. http://example.com should not be linkified.";
    let expected =
        "<p>Check out <a href=\"custom://example.com\">custom://example.com</a> for more info. http://example.com should not be linkified.</p>\n";
    let mut output = String::new();
    markdown_to_html(&mut output, source).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_strikethrough() {
    let path = data_path("strikethrough.txt");
    let s = fs::read_to_string(&path).expect("failed to read spec.json");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            allows_unsafe: true,
            xhtml: true,
            ..html::Options::default()
        },
        gfm(GfmOptions::default()),
        html::NO_EXTENSIONS,
    );
    suite.execute(&markdown_to_html)
}

#[test]
fn test_table() {
    let path = data_path("table.txt");
    let s = fs::read_to_string(&path).expect("failed to read spec.json");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            allows_unsafe: true,
            xhtml: true,
            ..html::Options::default()
        },
        gfm(GfmOptions::default()),
        html::NO_EXTENSIONS,
    );
    suite.execute(&markdown_to_html)
}

#[test]
fn test_task_list_item() {
    let path = data_path("task_list_item.txt");
    let s = fs::read_to_string(&path).expect("failed to read spec.json");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            allows_unsafe: true,
            xhtml: false,
            ..html::Options::default()
        },
        gfm(GfmOptions::default()),
        html::NO_EXTENSIONS,
    );
    suite.execute(&markdown_to_html)
}

#[test]
fn test_task_list_item_override() {
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options {
            allows_unsafe: true,
            xhtml: false,
            ..html::Options::default()
        },
        gfm(GfmOptions::default()),
        html::paragraph_renderer(html::ParagraphRendererOptions {
            render_task_list_item: Some(Box::new(
                |w, pr, _source, arena, list_ref, _context| -> Result<()> {
                    let task = as_kind_data!(arena, list_ref, ListItem).task().unwrap();
                    let css_class = match task {
                        Task::Unchecked => "task-list-item-unchecked",
                        Task::Checked => "task-list-item-checked",
                        _ => unreachable!(),
                    };
                    pr.writer().write_safe_str(w, "<span class=\"")?;
                    pr.writer().write(w, css_class)?;
                    pr.writer().write_safe_str(w, "\"></span>")?;
                    Ok(())
                },
            )),
            ..Default::default()
        }),
    );
    let source = r#"
- [ ] Task 1
- [x] Task 2
"#;
    let expected = r#"<ul>
<li><span class="task-list-item-unchecked"></span>Task 1</li>
<li><span class="task-list-item-checked"></span>Task 2</li>
</ul>
"#;
    let mut output = String::new();
    markdown_to_html(&mut output, source).unwrap();
    assert_eq!(output, expected);
}
