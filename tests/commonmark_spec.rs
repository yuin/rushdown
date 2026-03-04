use std::{fs, path::PathBuf};

use rushdown::{
    new_markdown_to_html, parser,
    renderer::html::{self, Options},
    test::{parse_case_env, MarkdownTestCase, MarkdownTestCaseOptions},
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Case {
    markdown: String,
    html: String,
    example: u32,
    start_line: u32,
    end_line: u32,
    section: String,
}

fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn test_commonmark_spec() {
    let path = data_path("spec.json");
    let s = fs::read_to_string(&path).expect("failed to read spec.json");
    let cases: Vec<Case> = serde_json::from_str(&s).expect("invalid cases.json");
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        Options {
            allows_unsafe: true,
            xhtml: true,
            ..Options::default()
        },
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    let target_cases = parse_case_env();

    for case in &cases {
        let description = format!("Example {}", case.example);
        let markdown = case.markdown.clone();
        let expected = case.html.clone();
        let no = case.example as u64;
        let test_case = MarkdownTestCase::new(
            no,
            description,
            markdown,
            expected,
            MarkdownTestCaseOptions::default(),
        );

        if cfg!(not(feature = "html-entities")) {
            match no {
                25 | 32 | 33 | 34 | 35 | 36 | 503 => continue,
                _ => (),
            }
        }

        if target_cases.contains(&no) || target_cases.is_empty() {
            test_case.execute(&markdown_to_html);
            println!("Test case {} passed", no);
        }
    }
}
