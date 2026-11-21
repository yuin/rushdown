//! Performance regression tests
//!
//! These tests exercise the quadratic-time parsing guards. They are only
//! compiled in release builds because debug builds are far too slow for the
//! input sizes used here (and would not be representative of production
//! performance). Run them with:
//!
//! ```text
//! cargo test --release --test perf
//! ```

#![cfg(not(debug_assertions))]

use std::time::{Duration, Instant};

use rushdown::{new_markdown_to_html, parser, renderer::html};

fn timeout() -> Duration {
    let multiplier = std::env::var("RUSHDOWN_TEST_TIMEOUT_MULTIPLIER")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0);
    Duration::from_secs_f64(5.0 * multiplier)
}

fn assert_fast(name: &str, source: &str, parser_options: parser::Options) {
    let markdown_to_html = new_markdown_to_html(
        parser_options,
        html::Options {
            allows_unsafe: true,
            xhtml: true,
            ..html::Options::default()
        },
        parser::NO_EXTENSIONS,
        html::NO_EXTENSIONS,
    );
    let mut output = String::new();
    let started = Instant::now();
    markdown_to_html(&mut output, source).expect("failed to render");
    let elapsed = started.elapsed();
    assert!(
        elapsed < timeout(),
        "{name} took too long: {elapsed:?} (limit: {:?})",
        timeout()
    );
}

#[test]
fn test_deep_nested_label_performance() {
    let n = 50_000;
    let source = format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert_fast(
        "Parsing deep nested labels",
        &source,
        parser::Options::default(),
    );
}

#[test]
fn test_pointy_bracket_link_destination_performance() {
    let n = 100_000;
    let source = "[a](<".repeat(n);
    assert_fast(
        "Parsing pointy-bracket link destinations",
        &source,
        parser::Options::default(),
    );
}

#[test]
fn test_many_processing_instruction_performance() {
    let n = 50_000;
    let source = format!("a {}", "<?".repeat(n));
    assert_fast(
        "Parsing processing instructions",
        &source,
        parser::Options::default(),
    );
}

#[test]
fn test_many_cdata_performance() {
    let n = 50_000;
    let source = "a <![CDATA[".repeat(n);
    assert_fast(
        "Parsing CDATA sections",
        &source,
        parser::Options::default(),
    );
}

#[test]
fn test_many_decl_performance() {
    let n = 50_000;
    let source = "a <!A ".repeat(n);
    assert_fast("Parsing declarations", &source, parser::Options::default());
}

#[test]
fn test_many_comment_performance() {
    let n = 50_000;
    let source = "a <!-- ".repeat(n);
    assert_fast("Parsing comments", &source, parser::Options::default());
}

#[test]
fn test_many_hyphens_performance() {
    let source = format!("{} x\n", "- ".repeat(100));
    assert_fast(
        "Parsing many hyphens",
        &source,
        parser::Options {
            auto_heading_ids: true,
            attributes: true,
            ..parser::Options::default()
        },
    );
}

/// Without the `openersBottom` guard, every unmatched closer scans all of the
/// openers before it, making this input quadratic.
#[test]
fn test_unmatched_delimiters_performance() {
    let n = 40_000;
    let source = format!("{}{}", "*a ".repeat(n), "a_ ".repeat(n));
    assert_fast(
        "Parsing unmatched delimiters",
        &source,
        parser::Options::default(),
    );
}
