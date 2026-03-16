use std::{fs, path::PathBuf};

use criterion::{criterion_group, criterion_main, Criterion};
use rushdown::{new_markdown_to_html, parser, renderer::html};
fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join(name)
}

fn criterion_benchmark(c: &mut Criterion) {
    let path = data_path("data.md");
    let s = fs::read_to_string(&path).expect("failed to read data.md");

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
    c.bench_function("rushdown-cached", |b| {
        b.iter(|| {
            let mut output = String::new();
            markdown_to_html(&mut output, s.as_str()).unwrap();
        })
    });
    c.bench_function("rushdown", |b| {
        b.iter(|| {
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
            let mut output = String::new();
            markdown_to_html(&mut output, s.as_str()).unwrap();
        })
    });
    c.bench_function("markdown-rs", |b| {
        b.iter(|| {
            markdown::to_html(s.as_str());
        })
    });
    c.bench_function("comrak", |b| {
        b.iter(|| {
            comrak::markdown_to_html(s.as_str(), &comrak::Options::default());
        })
    });
    c.bench_function("pulldown-cmark", |b| {
        b.iter(|| {
            let parser = pulldown_cmark::Parser::new(s.as_str());
            let mut html_output = String::new();
            pulldown_cmark::html::push_html(&mut html_output, parser);
            comrak::markdown_to_html(s.as_str(), &comrak::Options::default());
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
