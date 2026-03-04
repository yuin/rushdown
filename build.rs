use std::env;
use std::fs::{read_to_string, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

// gen_html_entities {{{

fn gen_html_entities() {
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("html_entities.rs");
    let mut file = BufWriter::new(File::create(&path).unwrap());

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest_dir.join("build/html_entities.txt");
    let mut m = phf_codegen::Map::new();

    let data = read_to_string(source).unwrap();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if let (Some(entity), Some(value)) = (parts.next(), parts.next()) {
            m.entry(entity, format!("\"{}\"", value));
        }
    }

    write!(
        &mut file,
        "static HTML_ENTITIES: phf::Map<&'static str, &'static str> = {}",
        m.build(),
    )
    .unwrap();
    writeln!(&mut file, ";").unwrap();
}

// }}}

// gen_unicode_case_foldings {{{

fn gen_unicode_case_foldings() {
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("unicode_case_foldings.rs");
    let mut file = BufWriter::new(File::create(&path).unwrap());

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest_dir.join("build/unicode_case_foldings.txt");
    let mut m = phf_codegen::Map::new();

    let data = read_to_string(source).unwrap();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if let (Some(from), Some(to)) = (parts.next(), parts.next()) {
            m.entry(decode_rust_u(from), format!("\"{}\"", to));
        }
    }
    write!(
        &mut file,
        "static UNICODE_CASE_FOLDINGS: phf::Map<char, &'static str> = {}",
        m.build(),
    )
    .unwrap();
    writeln!(&mut file, ";").unwrap();
}

// }}} gen_unicode_case_foldings

// gen_html_attributes {{{
//
fn gen_html_attributes() {
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("html_attributes.rs");
    let mut file = BufWriter::new(File::create(&path).unwrap());

    // Global attributes (HTML Living Standard / HTML5 global attributes set).
    // Keep these as raw literals; we will ASCII-sort later.
    let default_attrs: Vec<&str> = vec![
        "accesskey",
        "autocapitalize",
        "autofocus",
        "class",
        "contenteditable",
        "dir",
        "draggable",
        "enterkeyhint",
        "hidden",
        "id",
        "inert",
        "inputmode",
        "is",
        "itemid",
        "itemprop",
        "itemref",
        "itemscope",
        "itemtype",
        "lang",
        "part",
        "role",
        "slot",
        "spellcheck",
        "style",
        "tabindex",
        "title",
        "translate",
    ];

    // Element-specific attributes (fill this based on HTML5 spec).
    // name: element name in uppercase for const prefix, e.g. "PARAGRAPH"
    // attrs: attributes specific to that element, WITHOUT global/default ones.
    //
    // Example shown for <p>. (In practice <p> has no special attributes beyond global ones.)
    const ELEMENT_ATTRS: &[(&str, &[&str])] = &[
        ("PARAGRAPH", &[]),
        ("BLOCKQUOTE", &["cite"]),
        (
            "THEMATIC_BREAK",
            &["align", "color", "noshade", "size", "width"],
        ),
        ("LIST", &["reversed", "type"]),
        ("LIST_ITEM", &["value"]),
        (
            "LINK",
            &[
                "download",
                "href",
                "lang",
                "media",
                "ping",
                "referrerpolicy",
                "rel",
                "shape",
                "target",
            ],
        ),
        (
            "IMAGE",
            &[
                "align",
                "border",
                "crossorigin",
                "decoding",
                "height",
                "importance",
                "intrinsicsize",
                "ismap",
                "loading",
                "referrerpolicy",
                "sizes",
                "srcset",
                "usemap",
                "width",
            ],
        ),
        (
            "TABLE",
            &[
                "align",
                "bgcolor",
                "border",
                "cellpadding",
                "cellspacing",
                "frame",
                "rules",
                "summary",
                "width",
            ],
        ),
        (
            "TABLE_HEADER",
            &["align", "bgcolor", "char", "charoff", "valign"],
        ),
        (
            "TABLE_ROW",
            &["align", "bgcolor", "char", "charoff", "valign"],
        ),
        (
            "TABLE_CELL",
            &[
                "abbr", "align", "axis", "bgcolor", "char", "charoff", "colspan", "headers",
                "height", "rowspan", "scope", "valign", "width",
            ],
        ),
    ];

    // DEFAULT_ATTRS
    {
        let mut attrs = default_attrs.clone();
        attrs.sort_unstable(); // ASCII order
        attrs.dedup();
        file.write_all("/// List of HTML attributes common for all elements.\n".as_bytes())
            .unwrap();
        file.write_all("pub const DEFAULT_ATTRS: &str = \"".as_bytes())
            .unwrap();
        file.write_all(attrs.join(",").as_bytes()).unwrap();
        file.write_all("\";\n\n".as_bytes()).unwrap();
    }

    // Per-element filters
    for (name, specific) in ELEMENT_ATTRS {
        // Merge default + specific, sort ASCII, dedup
        let mut merged: Vec<&str> = Vec::with_capacity(default_attrs.len() + specific.len());
        merged.extend(default_attrs.iter().copied());
        merged.extend(specific.iter().copied());
        merged.sort_unstable();
        merged.dedup();

        writeln!(
            &mut file,
            "/// List of HTML attributes for {} elements.",
            name.to_ascii_lowercase()
        )
        .unwrap();
        write!(
            &mut file,
            "pub const {}_ATTRS : &str = \"{}\";\n\n",
            name,
            merged.join(",")
        )
        .unwrap();
    }
}

// }}} gen_html_attributes

// allowed_block_tags {{{
//
fn gen_allowed_block_tags() {
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("allowed_block_tags.rs");
    let mut file = BufWriter::new(File::create(&path).unwrap());
    let mut m = phf_codegen::Map::new();
    let block_tags = vec![
        "address",
        "article",
        "aside",
        "base",
        "basefont",
        "blockquote",
        "body",
        "caption",
        "center",
        "col",
        "colgroup",
        "dd",
        "details",
        "dialog",
        "dir",
        "div",
        "dl",
        "dt",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "frame",
        "frameset",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hr",
        "html",
        "iframe",
        "legend",
        "li",
        "link",
        "main",
        "menu",
        "menuitem",
        "meta",
        "nav",
        "noframes",
        "ol",
        "optgroup",
        "option",
        "p",
        "param",
        "search",
        "section",
        "summary",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "title",
        "tr",
        "track",
        "ul",
    ];
    for tag in block_tags {
        m.entry(tag, "true");
    }
    write!(
        &mut file,
        "static ALLOWED_BLOCK_TAGS: phf::Map<&'static str, bool> = {}",
        m.build(),
    )
    .unwrap();
    writeln!(&mut file, ";").unwrap();
}

// }}} gen_html_attributes

fn decode_rust_u(s: &str) -> char {
    let hex = s
        .strip_prefix(r"\u{")
        .and_then(|t| t.strip_suffix('}'))
        .ok_or_else(|| format!("bad s: {s}"))
        .unwrap();

    let cp = u32::from_str_radix(hex, 16)
        .map_err(|_| format!("bad hex: {s}"))
        .unwrap();

    let ch = char::from_u32(cp)
        .ok_or_else(|| format!("invalid Unicode scalar: {s}"))
        .unwrap();

    ch
}

fn main() {
    gen_html_entities();
    gen_unicode_case_foldings();
    gen_html_attributes();
    gen_allowed_block_tags();
}
