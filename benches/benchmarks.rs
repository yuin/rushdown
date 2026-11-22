use std::{fs, path::PathBuf};

use criterion::{criterion_group, criterion_main, Criterion};
use rushdown::{new_markdown_to_html, parser, renderer::html};
fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join(name)
}

fn parse_data_arg() -> Vec<String> {
    if let Some(files) = get_env("FILES") {
        files.split(',').map(|s| s.to_string()).collect()
    } else {
        vec!["data.md".to_string()]
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    let files = parse_data_arg();
    for file in files {
        println!("Benchmarking file: {}", file);
        let path = data_path(&file);
        let s = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("Failed to read file: {}", path.display()));

        let bname = |name: &str| -> String { format!("{}: {}", name, file) };

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
        c.bench_function(&bname("rushdown"), |b| {
            b.iter(|| {
                let mut output = String::new();
                markdown_to_html(&mut output, s.as_str()).unwrap();
            })
        });
        c.bench_function(&bname("markdown-rs"), |b| {
            b.iter(|| {
                markdown::to_html(s.as_str());
            })
        });
        c.bench_function(&bname("comrak"), |b| {
            b.iter(|| {
                comrak::markdown_to_html(s.as_str(), &comrak::Options::default());
            })
        });
        c.bench_function(&bname("pulldown-cmark"), |b| {
            b.iter(|| {
                let parser = pulldown_cmark::Parser::new(s.as_str());
                let mut html_output = String::new();
                pulldown_cmark::html::push_html(&mut html_output, parser);
                comrak::markdown_to_html(s.as_str(), &comrak::Options::default());
            })
        });
    }
}

#[allow(unreachable_code, unused)]
fn get_env(key: &str) -> Option<String> {
    #[cfg(feature = "no-std-unix-debug")]
    unsafe {
        extern crate libc;
        use core::slice;

        use libc::c_char;

        let mut buf = key.as_bytes().to_vec();
        buf.push(0);
        let ptr: *const c_char = buf.as_ptr() as *const c_char;

        let val = libc::getenv(ptr);
        if val.is_null() {
            return None;
        }

        let mut len: usize = 0;
        while *val.add(len) != (0 as libc::c_char) {
            len += 1;
        }

        let ret = String::from_utf8_lossy(slice::from_raw_parts(val as *const u8, len)).to_string();
        return Some(ret);
    }

    #[cfg(feature = "std")]
    {
        return std::env::var(key).ok();
    }

    None
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
