use pprof::protos::Message;
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};

use rushdown::{new_markdown_to_html, parser, renderer::html};

fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benches")
        .join("fixtures")
        .join(name)
}

fn main() {
    println!("start");

    {
        let guard = pprof::ProfilerGuardBuilder::default()
            .frequency(1000)
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build()
            .unwrap();
        let path = data_path("data.md");
        let s = fs::read_to_string(&path).expect("failed to read data.md");
        let markdown_to_html = new_markdown_to_html(
            parser::Options::default(),
            html::Options {
                allows_unsafe: true,
                xhtml: true,
                ..Default::default()
            },
            parser::NO_EXTENSIONS,
            html::NO_EXTENSIONS,
        );

        let start = std::time::Instant::now();
        while start.elapsed().as_secs_f64() < 10.0 {
            let mut out = String::new();
            markdown_to_html(&mut out, &s).unwrap()
        }

        match guard.report().build() {
            Ok(report) => {
                let mut file = File::create("profile.pb").unwrap();
                let profile = report.pprof().unwrap();

                let mut content = Vec::new();
                profile.encode(&mut content).unwrap();
                file.write_all(&content).unwrap();
            }
            Err(_) => {}
        };
    }
    println!("end");
}
