# rushdown
[![Tests](https://github.com/yuin/rushdown/actions/workflows/test.yml/badge.svg)](https://github.com/yuin/rushdown/actions/workflows/test.yml) [![Docs](https://docs.rs/rushdown/badge.svg)](https://docs.rs/rushdown) [![Crates.io](https://img.shields.io/crates/v/rushdown.svg?maxAge=2592000)](https://crates.io/crates/rushdown) ![Coverage](https://img.shields.io/endpoint?url=https://gist.githubusercontent.com/yuin/3c122e76a86b680d04700e14b3161f04/raw/rushdown-coverage.json)

A markdown parser written in Rust. Fast, Easy to extend, Standards-compliant.

rushdown is compliant with CommonMark 0.31.2 & [GitHub Flavored Markdown](https://github.github.com/gfm/)[^gfm-support].

[^gfm-support]: rushdown does not support [Disallowed Raw HTML](https://github.github.com/gfm/#disallowed-raw-html-extension-). 

## Motivation
I needed a Markdown parser that met the following requirements:

- Written in Rust
- Compliant with CommonMark
- Fast
- Extensible from the outside of the crate
- AST-based

In short, I wanted something like [goldmark](https://github.com/yuin/goldmark) written in Rust. However, no existing library satisfied these requirements.

## Features
- **Standards-compliant.**  rushdown is fully compliant with the latest [CommonMark](https://commonmark.org/) specification.
- **Extensible.**  Do you want to add a `@username` mention syntax to Markdown?
  You can easily do so in rushdown. You can add your AST nodes,
  parsers for block-level elements, parsers for inline-level elements,
  transformers for paragraphs, transformers for the whole AST structure, and
  renderers.
- **Performance.**  rushdown is one of the fastest CommonMark parser Rust implementations compared to [pulldown-cmark](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/), [comrak](https://docs.rs/comrak/latest/comrak/), and [markdown-rs](https://docs.rs/markdown/latest/markdown/).
- **Robust.**  rushdown is tested with `cargo fuzz`.
- **Built-in extensions.**  rushdown ships with GFM extensions.

## Benchmark
You can run this benchmark by `make bench`

rushdown builds a clean, extensible AST structure, achieves full compliance with CommonMark, all while being one of the fastest CommonMark parser implementation written in Rust.

```text
rushdown-cached         time:   [3.4741 ms 3.5533 ms 3.6504 ms]
rushdown                time:   [3.5167 ms 3.5838 ms 3.6607 ms]
markdown-rs             time:   [78.194 ms 79.865 ms 81.642 ms]
comrak                  time:   [3.9462 ms 4.0100 ms 4.0804 ms]
pulldown-cmark          time:   [5.4810 ms 5.5887 ms 5.7074 ms]
cmark                   time: 3.4892 ms
goldmark                time: 5.4065 ms
```

## Security
By default, rushdown does not render raw HTML or potentially-dangerous URLs.
If you need to gain more control over untrusted contents, it is recommended that you
use an HTML sanitizer such as [ammonia](https://docs.rs/ammonia/latest/ammonia/).

## Installation
Add dependency to your `Cargo.toml`:

```toml
[dependencies]
rushdown = "x.y.z"
```

CommonMark defines that parsers should handle [HTML entities](https://spec.commonmark.org/0.31.2/#entity-and-numeric-character-references) correctly. But this requires a large map that maps entity names to their corresponding Unicode code points. If you don't need this feature, you can disable it by adding the following line to your `Cargo.toml`:

```toml
rushdown = { version = "x.y.z", default-features = false, features = ["std"] }
```

In this case, the parser will only support numeric character references and some predefined entities (like `&amp;`, `&lt;`, `&gt;`, `&quot;`, etc).

rushdown can also be used in `no_std` environments. To enable this feature, add the following line to your `Cargo.toml`:

```toml
rushdown = { version = "x.y.z", default-features = false, features = ["no-std"] }
```

## Usage
### Basic Usage

Render Markdown(CommonMark, without GFM) to HTML string:

```rust
use rushdown::markdown_to_html_string;
let mut output = String::new();
let input = "# Hello, World!\n\nThis is a **Markdown** document.";
match markdown_to_html_string(&mut output, input) {
    Ok(_) => {
        println!("HTML output:\n{}", output);
    }
    Err(e) => {
    println!("Error: {:?}", e);
    }
 };
```

Render Markdown with GFM extensions to HTML string:

```rust
use core::fmt::Write;
use rushdown::{
    new_markdown_to_html,
    parser::{self, ParserExtension, GfmOptions},
    renderer::html::{self, RendererExtension},
    Result,
};

let markdown_to_html = new_markdown_to_html(
    parser::Options::default(),
    html::Options::default(),
    parser::gfm(GfmOptions::default()),
    html::NO_EXTENSIONS,
);
let mut output = String::new();
let input = "# Hello, World!\n\nThis is a ~~Markdown~~ document.";
match markdown_to_html(&mut output, input) {
    Ok(_) => {
        println!("HTML output:\n{}", output);
    }
    Err(e) => {
        println!("Error: {:?}", e);
    }
}
```

You can use subset of the GFM extensions:

```rust
use core::fmt::Write;
use rushdown::{
    new_markdown_to_html,
    parser::{self, ParserExtension},
    renderer::html::{self, RendererExtension},
    Result,
};

let markdown_to_html = new_markdown_to_html(
    parser::Options::default(),
    html::Options::default(),
    parser::gfm_table().and(parser::gfm_task_list_item()),
    html::NO_EXTENSIONS,
);
let mut output = String::new();
let input = "# Hello, World!\n\nThis is a **Markdown** document.";
match markdown_to_html(&mut output, input) {
    Ok(_) => {
        println!("HTML output:\n{}", output);
    }
    Err(e) => {
        println!("Error: {:?}", e);
    }
}
```


### Parser options

| Option | Default value | Description |
| --- | --- | --- |
| `attributes` | `false` | Whether to parse attributes. |
| `auto_heading_ids` | `false` | Whether to automatically generate heading IDs. |
| `without_default_parsers` | `false` | Whether to disable default parsers. |
| `arena` | `ArenaOptions::default()` | Options for the arena allocator. |
| `escaped_space` | `false` | If true, a '\' escaped half-space(0x20) will not trigger parsers. |
| `id_generator` : `None`(BasicNodeIdGenerator) | An ID generator for generating node IDs. |

Currently only headings support attributes.
Attributes are being discussed in the [CommonMark forum](https://talk.commonmark.org/t/consistent-attribute-syntax/272). This syntax may possibly change in the future.

```markdown
## heading ## {#id .className attrName=attrValue class="class1 class2"}

## heading {#id .className attrName=attrValue class="class1 class2"}

heading {#id .className attrName=attrValue}
============
```

#### Arena options

| Option | Default value | Description |
| --- | --- | --- |
| `initial_size` | `1024` | The initial capacity of the arena. |

### GFM Parser options

| Option | Default value | Description |
| --- | --- | --- |
| `linkify` | `LinkifyOptions::default()` | Options for linkify extension. |

#### Linkify options

| Option | Default value | Description |
| --- | --- | --- |
| `allowed_protocols` | `["http", "https", "ftp", "mailto"]` | A list of allowed protocols for linkification. |
| `url_scanner` | default function | A function that scans a string for URLs. |
| `www_scanner` | default function | A function that scans a string for www links. |
| `email_scanner` | default function | A function that scans a string for email addresses. |

### HTML Renderer options

| Option | Default value | Description |
| --- | --- | --- |
| `hard_wrap` | `false` | Renders soft line breaks as hard line breaks (`<br />`). |
| `xhtml` | `false` | Whether to render HTML in XHTML style. |
| `allows_unsafe` | `false` | Whether to allow rendering raw HTML and potentially-dangerous URLs. |
| `escaped_space` | `false` | Indicates that a '\' escaped half-space(0x20) should not be rendered. |
| `attribute_filters` | default filters | A list of filters for rendering attributes as HTML tag attributes. |

#### Customize Task list item rendering
[GFM](https://github.github.com/gfm/#task-list-items-extension-) does not define details how task list items should be rendered. 

You can customize the rendering of task list items by implementing a function:

```rust
use rushdown::{
    ast, new_markdown_to_html_string,
    parser::{self, GfmOptions},
    renderer,
    renderer::html,
};

let markdown_to_html = new_markdown_to_html_string(
    parser::Options::default(),
    html::Options::default(),
    parser::gfm(GfmOptions::default()),
    html::paragraph_renderer(html::ParagraphRendererOptions {
        render_task_list_item: Some(Box::new(
            |w: &mut String,
             pr: &html::ParagraphRenderer<String>,
             source: &str,
             arena: &ast::Arena,
             node_ref: ast::NodeRef,
             ctx: &mut renderer::Context| {
                // do stuff
                Ok(())
            },
        )),
        ..Default::default()
    }),
);
let input = r#"
- [ ] Item
- [x] Item
"#;
let mut output = String::new();
match markdown_to_html(&mut output, input) {
    Ok(_) => {
        println!("HTML output:\n{}", output);
    }
    Err(e) => {
        println!("Error: {:?}", e);
    }
}
```

## AST
rushdown builds a clean AST structure that is easy to traverse and manipulate. The AST is built on top of an arena allocator, which allows for efficient memory management and fast node access.

Each node belongs to a specific type and kind.

- Node
   - has a `type_data`: node type(block or inline) specific data
   - has a `kind_data`: node kind(e.g. Text, Paragraph) specific data
   - has a `parent`, `first_child`, `next_sibling`... : relationships

These macros can be used to access node data.

- `matches_kind!` - Helper macro to match kind data.
- `as_type_data!` - Helper macro to downcast type data.
- `as_type_data_mut!` - Helper macro to downcast mutable type data.
- `as_kind_data!` - Helper macro to downcast kind data.
- `as_kind_data_mut!` - Helper macro to downcast mutable kind data.
- `matches_extension_kind!` - Helper macro to match extension kind.
- `as_extension_data!` - Helper macro to downcast extension data.
- `as_extension_data_mut!` - Helper macro to downcast mutable extension data.

`*kind*` and `*type*` macros are defined for rushdown builtin nodes.
`*extension*` macros are defined for [extension](#extending-rushdown) nodes.

Nodes are stored in an arena for efficient memory management and access.
Each node is identified by a `NodeRef`, which contains the index and unique ID of the node.

You can get and manipulate nodes using the `Arena` and its methods.

```rust
use rushdown::ast::*;
use rushdown::{as_type_data_mut, as_type_data, as_kind_data};
use rushdown::text::Segment;

let mut arena = Arena::new();
let source = "Hello, World!";
let doc_ref = arena.new_node(Document::new());
let paragraph_ref = arena.new_node(Paragraph::new());
let seg = Segment::new(0, source.len());
as_type_data_mut!(&mut arena[paragraph_ref], Block).append_line(seg);
let text_ref = arena.new_node(Text::new(seg));
paragraph_ref.append_child(&mut arena, text_ref);
doc_ref.append_child(&mut arena, paragraph_ref);

assert_eq!(arena[paragraph_ref].first_child().unwrap(), text_ref);
assert_eq!(
    as_kind_data!(&arena[text_ref], Text).str(source),
    "Hello, World!"
);
assert_eq!(
    as_type_data!(&arena[paragraph_ref], Block)
        .lines()
        .first()
        .unwrap()
        .str(source),
    "Hello, World!"
);
```

Walkng the AST: You can not mutate the AST while walking it. If you want to mutate the AST, collect the node refs and mutate them after walking.

```rust
use core::result::Result;
use core::error::Error;
use core::fmt::{self, Display, Formatter};
use rushdown::ast::*;
use rushdown::matches_kind;

#[derive(Debug)]
enum UserError { SomeError(&'static str) }

impl Error for UserError {}

impl Display for UserError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self { UserError::SomeError(msg) => write!(f, "UserError: {}", msg) }
    }
}

let mut arena = Arena::default();
let doc_ref = arena.new_node(Document::new());
let paragraph_ref1 = arena.new_node(Paragraph::new());
let text1 = arena.new_node(Text::new("Hello, World!"));
let paragraph_ref2 = arena.new_node(Paragraph::new());
let text2 = arena.new_node(Text::new("This is a test."));

doc_ref.append_child(&mut arena, paragraph_ref1);
paragraph_ref1.append_child(&mut arena, text1);
doc_ref.append_child(&mut arena, paragraph_ref2);
paragraph_ref2.append_child(&mut arena, text2);

let mut target: Option<NodeRef> = None;

walk(&arena, doc_ref, &mut |arena: &Arena,
                            node_ref: NodeRef,
                            entering: bool| -> Result<WalkStatus, UserError > {
    if entering {
        if let Some(fc) = arena[node_ref].first_child() {
            if let KindData::Text(t) = &arena[fc].kind_data() {
                if t.str("").contains("test") {
                    target = Some(node_ref);
                }
                if t.str("").contains("error") {
                    return Err(UserError::SomeError("Some error occurred"));
                }
            }
        }
    }
    Ok(WalkStatus::Continue)
}).ok();
assert_eq!(target, Some(paragraph_ref2));
```

## Extending rushdown <a name="extending-rushdown"></a>
See `tests/extension.rs` and `override_renderer.rs` for examples of how to extend rushdown.

You can extend rushdown by implementing AST nodes, custom block/inline parsers, transformers, and renderers.

The key point of rushdown extensibility is 'dynamic parser/renderer constructor injection'.

You can add parsers and renderers like the following:

```text
fn user_mention_parser_extension() -> impl ParserExtension {
    ParserExtensionFn::new(|p: &mut Parser| {
        p.add_inline_parser(
            UserMentionParser::new,
            NoParserOptions, // no options for this parser
            PRIORITY_EMPHASIS + 100,
        );
    })
}

fn user_mention_html_renderer_extension<'cb, W>(
    options: UserMentionOptions,
) -> impl RendererExtension<'cb, W>
where
    W: TextWrite + 'cb,
{
    RendererExtensionFn::new(move |r: &mut Renderer<'cb, W>| {
        r.add_node_renderer(UserMentionHtmlRenderer::with_options, options);
    })
}
```

`UserMentionParser::new` is a constructor function that returns a `UserMentionParser` instance. rushdown will call this function with the necessary arguments.

Parser/Transformer constructor function can take these arguments if needed, in any order:

- `rushdown::parser::Options`
- parser options defined by the user
- `Rc<RefCell<rushdown::parser::ContextKeyRegistry>>`

HtmlRenderer constructor function can take these arguments if needed, in any order:

- `rushdown::renderer::html::Options`
- renderer options defined by the user
- `Rc<RefCell<rushdown::renderer::ContextKeyRegistry>>`
- `Rc<RefCell<rushdown::renderer::NodeKindRegistry>>`

## Donation
BTC: 1NEDSyUmo4SMTDP83JJQSWi1MvQUGGNMZB

Github sponsors also welcome.

## License
MIT

## Author
Yusuke Inuzuka
