use rushdown::{
    ast::pretty_print,
    new_markdown_to_html,
    parser::{
        self, LinkifyOptions, Parser, ParserExtension, empty_parser_extension, gfm_linkify,
        gfm_strikethrough, gfm_table, gfm_task_list_item, parser_extension,
    },
    renderer::html,
    text::BasicReader,
};
use wasm_bindgen::prelude::*;

const OPT_TABLE: i32 = 1 << 0;
const OPT_TASK_LIST_ITEM: i32 = 1 << 1;
const OPT_STRIKETHROUGH: i32 = 1 << 2;
const OPT_LINKIFY: i32 = 1 << 3;
const OPT_UNSAFE: i32 = 1 << 16;

#[wasm_bindgen]
pub fn markdown_to_html(src: &str, options: i32) -> String {
    let pext = to_parser_extension(options);
    let hopts = to_html_renderer_options(options);

    let markdown_to_html =
        new_markdown_to_html(parser::Options::default(), hopts, pext, html::NO_EXTENSIONS);
    let mut out = String::new();
    markdown_to_html(&mut out, src).unwrap();

    out
}

#[wasm_bindgen]
pub fn markdown_to_ast(src: &str, options: i32) -> String {
    let pext = to_parser_extension(options);

    let mut reader = BasicReader::new(src);
    let parser = Parser::with_extensions(parser::Options::default(), pext);
    let (arena, document_ref) = parser.parse(&mut reader);
    let mut out = String::new();
    pretty_print(&mut out, &arena, document_ref, src).unwrap();
    out
}

fn to_html_renderer_options(options: i32) -> html::Options {
    let mut opts = html::Options::default();
    if options & OPT_UNSAFE != 0 {
        opts.allows_unsafe = true;
    }
    opts
}

fn to_parser_extension(options: i32) -> impl ParserExtension {
    empty_parser_extension()
        .and(parser_extension(move |p| {
            if options & OPT_TABLE != 0 {
                gfm_table().apply(p)
            }
        }))
        .and(parser_extension(move |p| {
            if options & OPT_TASK_LIST_ITEM != 0 {
                gfm_task_list_item().apply(p)
            }
        }))
        .and(parser_extension(move |p| {
            if options & OPT_STRIKETHROUGH != 0 {
                gfm_strikethrough().apply(p)
            }
        }))
        .and(parser_extension(move |p| {
            if options & OPT_LINKIFY != 0 {
                gfm_linkify(LinkifyOptions::default()).apply(p)
            }
        }))
}
