#![doc = include_str!("../README.md")]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod ast;
pub mod context;
pub mod parser;
pub mod renderer;
pub mod test;
pub mod text;
pub mod util;

#[cfg(feature = "html-entities")]
mod html_entity;

mod scanner;

mod error;
use alloc::string::String;
pub use error::Error;
pub use error::Result;

use crate::parser::Parser;
use crate::parser::ParserExtension;
use crate::renderer::html;
use crate::renderer::TextWrite;
use crate::text::BasicReader;

/// Trait for converting Markdown to HTML.
///
/// # Errors
/// Parsing phase will never fail, so the only possible errors are I/O errors during rendering.
pub trait MarkdownToHtml<W: TextWrite = String> {
    /// Converts the given Markdown source to HTML and writes it to the output.
    fn markdown_to_html(&self, out: &mut W, source: &str) -> Result<()>;
}

impl<W: TextWrite, F> MarkdownToHtml<W> for F
where
    F: Fn(&mut W, &str) -> Result<()>,
{
    fn markdown_to_html(&self, out: &mut W, source: &str) -> Result<()> {
        (self)(out, source)
    }
}

/// Creates a function that converts Markdown to HTML using the specified parser and renderer.
///
/// # Arguments
/// - `parser_options`: Options for the Markdown parser.
/// - `renderer_options`: Options for the HTML renderer.
/// - `parser_extension`: Extension for the Markdown parser. If no extensions are needed, use [`crate::parser::NO_EXTENSIONS`].
/// - `renderer_extension`: Extension for the HTML renderer. If no extensions are needed, use [`crate::renderer::html::NO_EXTENSIONS`].
///
/// # Examples
/// ```
/// use core::fmt::Write;
/// use rushdown::{
///     new_markdown_to_html,
///     parser::{self, ParserExtension},
///     renderer::html::{self, RendererExtension},
///     Result,
/// };
///
/// let markdown_to_html = new_markdown_to_html(
///     parser::Options::default(),
///     html::Options::default(),
///     parser::gfm_table().and(parser::gfm_task_list_item()),
///     html::NO_EXTENSIONS,
/// );
/// let mut output = String::new();
/// let input = "# Hello, World!\n\nThis is a **Markdown** document.";
/// match markdown_to_html(&mut output, input) {
///     Ok(_) => {
///         println!("HTML output:\n{}", output);
///     }
///     Err(e) => {
///         println!("Error: {:?}", e);
///     }
/// }
/// ```
pub fn new_markdown_to_html<'r, W>(
    parser_options: parser::Options,
    renderer_options: html::Options,
    parser_extension: impl ParserExtension,
    renderer_extension: impl html::RendererExtension<'r, W>,
) -> impl Fn(&mut W, &str) -> Result<()> + 'r
where
    W: TextWrite + 'r,
{
    let parser = Parser::with_extensions(parser_options, parser_extension);
    let renderer = html::Renderer::<'r, W>::with_extensions(renderer_options, renderer_extension);
    move |output: &mut W, source: &str| {
        let mut reader = BasicReader::new(source);
        let (arena, document_ref) = parser.parse(&mut reader);
        renderer.render(output, source, &arena, document_ref)
    }
}

/// Creates a function that converts Markdown to HTML using the specified parser and renderer,
/// with output written to a `String`.
pub fn new_markdown_to_html_string<'r>(
    parser_options: parser::Options,
    renderer_options: html::Options,
    parser_extension: impl ParserExtension,
    renderer_extension: impl html::RendererExtension<'r, String>,
) -> impl Fn(&mut String, &str) -> Result<()> + 'r {
    new_markdown_to_html::<String>(
        parser_options,
        renderer_options,
        parser_extension,
        renderer_extension,
    )
}

/// Converts Markdown(CommonMark) to HTML using default parser and renderer options.
///
/// # Examples
/// ```
/// use rushdown::markdown_to_html_string;
/// let mut output = String::new();
/// let input = "# Hello, World!\n\nThis is a **Markdown** document.";
/// match markdown_to_html_string(&mut output, input) {
///     Ok(_) => {
///         println!("HTML output:\n{}", output);
///     }
///     Err(e) => {
///     println!("Error: {:?}", e);
///     }
///  };
///  ```
pub fn markdown_to_html_string(output: &mut String, source: &str) -> Result<()> {
    let parser = Parser::with_options(parser::Options::default());
    let renderer = html::Renderer::with_options(html::Options::default());
    let mut reader = BasicReader::new(source);
    let (arena, document_ref) = parser.parse(&mut reader);
    renderer.render(output, source, &arena, document_ref)
}

// macros {{{

/// Helper macro to match kind data.
///
/// # Examples
/// ```
/// use rushdown::ast::{Arena, NodeRef, KindData, Paragraph};
/// use rushdown::matches_kind;
///
/// let mut arena = Arena::new();
/// let para_ref: NodeRef = arena.new_node(Paragraph::new());
/// assert!(matches_kind!(arena, para_ref, Paragraph));
/// assert!(matches_kind!(arena[para_ref], Paragraph));
/// ```
#[macro_export]
macro_rules! matches_kind {
    ($arena:expr, $node_ref:expr, $variant:ident) => {
        matches!(
            $arena[$node_ref].kind_data(),
            $crate::ast::KindData::$variant(_)
        )
    };
    ($node:expr, $variant:ident) => {
        matches!($node.kind_data(), $crate::ast::KindData::$variant(_))
    };
}

/// Helper macro to match extension kind.
///
/// # Examples
/// ```
/// use core::fmt::{self, Write};
/// use rushdown::ast::{Arena, NodeRef, NodeType, NodeKind, KindData, PrettyPrint, pp_indent};
/// use rushdown::matches_extension_kind;
///
/// #[derive(Debug)]
/// struct Admonition {
///     kind: String,
/// }
///
/// impl NodeKind for Admonition {
///     fn typ(&self) -> NodeType { NodeType::ContainerBlock }
///
///     fn kind_name(&self) -> &'static str { "Admonition" }
/// }
///
/// impl PrettyPrint for Admonition {
///     fn pretty_print(&self, w: &mut dyn Write, _source: &str, level: usize) -> fmt::Result {
///         writeln!(w, "{}kind: {}", pp_indent(level), self.kind)
///     }
/// }
///
/// impl From<Admonition> for KindData {
///     fn from(e: Admonition) -> Self { KindData::Extension(Box::new(e)) }
/// }
///
/// let mut arena = Arena::new();
/// let ext_ref: NodeRef = arena.new_node(Admonition{kind: "note".to_string()});
/// assert!(matches_extension_kind!(arena, ext_ref, Admonition));
/// assert!(matches_extension_kind!(arena[ext_ref], Admonition));
/// ```
///
#[macro_export]
macro_rules! matches_extension_kind {
    ($arena:expr, $ref:expr, $ext_type:ty) => {
        (if let $crate::ast::KindData::Extension(ref d) = $arena[$ref].kind_data() {
            (d.as_ref() as &dyn ::core::any::Any)
                .downcast_ref::<$ext_type>()
                .is_some()
        } else {
            false
        })
    };
    ($node:expr, $ext_type:ty) => {
        (if let $crate::ast::KindData::Extension(ref d) = $node.kind_data() {
            (d.as_ref() as &dyn ::core::any::Any)
                .downcast_ref::<$ext_type>()
                .is_some()
        } else {
            false
        })
    };
}

/// Helper macro to downcast extension data.
///
/// # Examples
/// ```
/// use core::fmt::{self, Write};
/// use rushdown::ast::{Arena, NodeRef, NodeType, NodeKind, KindData, PrettyPrint, pp_indent};
/// use rushdown::as_extension_data;
///
/// #[derive(Debug)]
/// struct Admonition {
///     kind: String,
/// }
///
/// impl NodeKind for Admonition {
///     fn typ(&self) -> NodeType { NodeType::ContainerBlock }
///
///     fn kind_name(&self) -> &'static str { "Admonition" }
/// }
///
/// impl PrettyPrint for Admonition {
///     fn pretty_print(&self, w: &mut dyn Write, _source: &str, level: usize) -> fmt::Result {
///         writeln!(w, "{}kind: {}", pp_indent(level), self.kind)
///     }
/// }
///
/// impl From<Admonition> for KindData {
///     fn from(e: Admonition) -> Self { KindData::Extension(Box::new(e)) }
/// }
///
/// let mut arena = Arena::new();
/// let ext_ref: NodeRef = arena.new_node(Admonition{kind: "note".to_string()});
/// let ext_data = as_extension_data!(arena, ext_ref, Admonition);
/// assert_eq!(ext_data.kind, "note");
/// let ext_data = as_extension_data!(arena[ext_ref], Admonition);
/// assert_eq!(ext_data.kind, "note");
/// ```
///
#[macro_export]
macro_rules! as_extension_data {
    ($arena:expr, $ref:expr, $ext_type:ty) => {
        (if let $crate::ast::KindData::Extension(ref d) = $arena[$ref].kind_data() {
            (d.as_ref() as &dyn ::core::any::Any)
                .downcast_ref::<$ext_type>()
                .expect("Failed to downcast extension data")
        } else {
            panic!("Node is not an extension node")
        })
    };
    ($node:expr, $ext_type:ty) => {
        (if let $crate::ast::KindData::Extension(ref d) = $node.kind_data() {
            (d.as_ref() as &dyn ::core::any::Any)
                .downcast_ref::<$ext_type>()
                .expect("Failed to downcast extension data")
        } else {
            panic!("Node is not an extension node")
        })
    };
}

/// Helper macro to downcast mutable extension data.
///
/// See [`as_extension_data!`] for examples.
#[macro_export]
macro_rules! as_extension_data_mut {
    ($arena:expr, $ref:expr, $ext_type:ty) => {
        (if let $crate::ast::KindData::Extension(ref mut d) = $arena[$ref].kind_data_mut() {
            (d.as_mut() as &mut dyn ::core::any::Any)
                .downcast_mut::<$ext_type>()
                .expect("Failed to downcast extension data")
        } else {
            panic!("Node is not an extension node")
        })
    };
    ($node:expr, $ext_type:ty) => {
        (if let $crate::ast::KindData::Extension(ref mut d) = $node.kind_data_mut() {
            (d.as_mut() as &mut dyn ::core::any::Any)
                .downcast_mut::<$ext_type>()
                .expect("Failed to downcast extension data")
        } else {
            panic!("Node is not an extension node")
        })
    };
}

/// Helper macro to work with kind data.
///
/// # Examples
/// ```
/// use rushdown::ast::{Arena, NodeRef, KindData, Emphasis};
/// use rushdown::as_kind_data;
///
/// let mut arena = Arena::new();
/// let para_ref: NodeRef = arena.new_node(Emphasis::new(1));
/// let data = as_kind_data!(arena, para_ref, Emphasis);
/// assert_eq!(data.level(), 1);
/// let data = as_kind_data!(arena[para_ref], Emphasis);
/// assert_eq!(data.level(), 1);
/// ```
#[macro_export]
macro_rules! as_kind_data {
    ($arena:expr, $node_ref:expr, $variant:ident) => {
        (if let $crate::ast::KindData::$variant(ref d) = $arena[$node_ref].kind_data() {
            d
        } else {
            panic!(
                "Expected kind data variant {} but found {:?}",
                stringify!($variant),
                $arena[$node_ref].kind_data()
            )
        })
    };
    ($node:expr, $variant:ident) => {
        (if let $crate::ast::KindData::$variant(ref d) = $node.kind_data() {
            d
        } else {
            panic!(
                "Expected kind data variant {} but found {:?}",
                stringify!($variant),
                $node.kind_data()
            )
        })
    };
}

/// Helper macro to work with mutable kind data.
///
/// See [`as_kind_data!`] for examples.
#[macro_export]
macro_rules! as_kind_data_mut {
    ($arena:expr, $node_ref:expr, $variant:ident) => {
        (if let $crate::ast::KindData::$variant(ref mut d) = $arena[$node_ref].kind_data_mut() {
            d
        } else {
            panic!(
                "Expected kind data variant {} but found {:?}",
                stringify!($variant),
                $arena[$node_ref].kind_data()
            )
        })
    };
    ($node:expr, $variant:ident) => {
        (if let $crate::ast::KindData::$variant(ref mut d) = $node.kind_data_mut() {
            d
        } else {
            panic!(
                "Expected kind data variant {} but found {:?}",
                stringify!($variant),
                $node.kind_data()
            )
        })
    };
}

/// Helper macro to work with type data.
///
/// # Examples
/// ```
/// use rushdown::ast::{Arena, NodeRef, TypeData, Block, Paragraph};
/// use rushdown::as_type_data;
///
/// let mut arena = Arena::new();
/// let para_ref: NodeRef = arena.new_node(Paragraph::new());
/// let data = as_type_data!(arena, para_ref, Block);
/// assert!(data.lines().is_empty());
/// let data = as_type_data!(arena[para_ref], Block);
/// assert!(data.lines().is_empty());
/// ```
///
#[macro_export]
macro_rules! as_type_data {
    ($arena:expr, $node_ref:expr, $variant:ident) => {
        (if let $crate::ast::TypeData::$variant(ref d) = $arena[$node_ref].type_data() {
            d
        } else {
            panic!(
                "Expected type data variant {} but found {:?}",
                stringify!($variant),
                $arena[$node_ref].type_data()
            )
        })
    };
    ($node:expr, $variant:ident) => {
        (if let $crate::ast::TypeData::$variant(ref d) = $node.type_data() {
            d
        } else {
            panic!(
                "Expected type data variant {} but found {:?}",
                stringify!($variant),
                $node.type_data()
            )
        })
    };
}

/// Helper macro to work with mutable type data.
///
/// See [`as_type_data!`] for examples.
#[macro_export]
macro_rules! as_type_data_mut {
    ($arena:expr, $node_ref:expr, $variant:ident) => {
        (if let $crate::ast::TypeData::$variant(ref mut d) = $arena[$node_ref].type_data_mut() {
            d
        } else {
            panic!(
                "Expected type data variant {} but found {:?}",
                stringify!($variant),
                $arena[$node_ref].type_data()
            )
        })
    };
    ($node:expr, $variant:ident) => {
        (if let $crate::ast::TypeData::$variant(ref mut d) = $node.type_data_mut() {
            d
        } else {
            panic!(
                "Expected type data variant {} but found {:?}",
                stringify!($variant),
                $node.type_data()
            )
        })
    };
}

// }}} macros

// debug stuff {{{

#[cfg(not(feature = "std"))]
pub mod debug {
    #[cfg(feature = "no-std-unix-debug")]
    extern crate libc;

    use core::fmt::{self, Write};

    #[allow(dead_code)]
    pub struct Stdout;

    impl Write for Stdout {
        #[allow(unreachable_code, unused)]
        fn write_str(&mut self, s: &str) -> fmt::Result {
            #[cfg(feature = "no-std-unix-debug")]
            unsafe {
                libc::write(1, s.as_ptr() as *const _, s.len());
            }
            Ok(())
        }
    }

    #[macro_export]
    macro_rules! print {
        ($($arg:tt)*) => {{
            use core::fmt::Write;
            let mut out = $crate::debug::Stdout;
            core::write!(&mut out, $($arg)*).ok();
        }};
    }

    #[macro_export]
    macro_rules! println {
        ($($arg:tt)*) => {{
            $crate::print!("{}\n", format_args!($($arg)*));
        }};
    }
}
// }}}
