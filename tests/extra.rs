use std::{fs, path::PathBuf};

use rushdown::{new_markdown_to_html, parser, renderer::html, test::MarkdownTestSuite};

fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn test_extra() {
    let path = data_path("extra.txt");
    let s = fs::read_to_string(&path).expect("failed to read spec.json");
    let suite = MarkdownTestSuite::with_str(s.as_str()).unwrap();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
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

// #[test]
// fn test_fuzz() {
//     let path = "fuzz/artifacts/markdown/crash-ca6b858640652797bd4672de9b731f984e104523";
//     let s = std::fs::read_to_string(path).unwrap();
//     let markdown_to_html = new_markdown_to_html(
//         parser::Options::default(),
//         html::Options::default(),
//         parser::gfm(parser::GfmOptions::default()),
//         html::NO_EXTENSIONS,
//     );
//     let mut output = String::new();
//     let _ = markdown_to_html(&mut output, s.as_str());
//     println!("{}", output);
// }
