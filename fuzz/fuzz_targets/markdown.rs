#![no_main]

use libfuzzer_sys::fuzz_target;
use rushdown::{new_markdown_to_html, parser, renderer::html};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let markdown_to_html = new_markdown_to_html(
            parser::Options::default(),
            html::Options::default(),
            parser::gfm(parser::GfmOptions::default()),
            html::NO_EXTENSIONS,
        );
        let mut output = String::new();
        let _ = markdown_to_html(&mut output, s);
    }
});
