//! Built-in parsers.

mod attribute;
pub use self::attribute::*;

mod paragraph;
pub use self::paragraph::*;

mod blockquote;
pub use self::blockquote::*;

mod code_block;
pub use self::code_block::*;

mod heading;
pub use self::heading::*;

mod thematic_break;
pub use self::thematic_break::*;

mod list;
pub use self::list::*;

mod html_block;
pub use self::html_block::*;

mod table;
pub use self::table::*;

mod linkify;
pub use self::linkify::*;

mod code_span;
pub use self::code_span::*;

mod raw_html;
pub use self::raw_html::*;

mod delimiter;
pub use self::delimiter::*;

mod emphasis;
pub use self::emphasis::*;

mod link;
pub use self::link::*;

mod link_ref;
pub use self::link_ref::*;

mod auto_link;
pub use self::auto_link::*;

mod strikethrough;
pub use self::strikethrough::*;

mod task_list_item;
pub use self::task_list_item::*;

extern crate alloc;

use core::cell::Cell;
use core::cell::RefCell;
use core::fmt;
use core::fmt::Debug;
use core::iter;
use core::marker::PhantomData;

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::as_kind_data;
use crate::as_type_data_mut;
use crate::ast::KindData;
use crate::ast::Text;
use crate::ast::TextQualifier;
use crate::ast::NODE_REF_UNDEFINED;
use crate::ast::{self, Arena, ArenaOptions, NodeRef};
use crate::context;
use crate::context::AnyValueSpec;
use crate::context::ContextKey;
use crate::context::ContextKeyRegistry;
use crate::error::{CallbackError, Result};
use crate::matches_kind;
use crate::text;
use crate::text::Reader;
use crate::util;
use crate::util::fold_case_full;
use crate::util::is_blank;
use crate::util::is_punct;
use crate::util::is_space;
use crate::util::to_link_reference;
use crate::util::trim_left;
use crate::util::trim_left_space;
use crate::util::trim_right;
use crate::util::trim_right_space;
use crate::util::utf8_len;
use crate::util::{HashMap, HashSet, Prioritized};

use bitflags::bitflags;

#[allow(unused_imports)]
#[cfg(not(feature = "std"))]
use crate::println;

// Context {{{

//   NodeIdGenerator {{{

/// A trait for generating unique node IDs.
pub trait GenerateNodeId {
    /// Generates a unique ID for the given value and node kind.
    /// Implementations must ensure that the generated ID is unique within the given `ids`.
    fn generate_node_id(&self, value: &str, data: &ast::KindData, ids: &NodeIds) -> String;
}

/// A basic implementation of [`GenerateNodeId`].
#[derive(Default, Debug)]
struct BasicNodeIdGenerator;

impl BasicNodeIdGenerator {
    pub fn new() -> Self {
        BasicNodeIdGenerator {}
    }
}

impl GenerateNodeId for BasicNodeIdGenerator {
    fn generate_node_id(&self, value: &str, data: &ast::KindData, ids: &NodeIds) -> String {
        let mut b = trim_left_space(value.as_bytes());
        b = trim_right_space(b);
        b = trim_left(b, b"0123456789-_");
        b = trim_right(b, b"-_");
        let cw = fold_case_full(b);
        let mut result = String::new();
        let mut iter = cw.iter().peekable();
        while let Some(&c) = iter.next() {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                result.push(c as char);
            } else if is_space(c) || c == b'-' || c == b'_' {
                while let Some(&&n) = iter.peek() {
                    if is_space(n) || n == b'-' || n == b'_' {
                        iter.next();
                    } else {
                        break;
                    }
                }
                result.push('-');
            } else if let Some(ulen) = utf8_len(c) {
                if ulen != 1 {
                    let mut buf = [0u8; 4];
                    buf[0] = c;
                    let mut i = 0;
                    while i < ulen - 1 {
                        if let Some(&&n) = iter.peek() {
                            buf[i + 1] = n;
                            iter.next();
                        }
                        i += 1;
                    }
                    if let Ok(s) = str::from_utf8(&buf[..ulen]) {
                        result.push_str(s);
                    }
                }
            }
        }
        if result.is_empty() {
            result.push_str(if matches!(data, KindData::Heading(_)) {
                "heading"
            } else {
                "id"
            });
        }
        if !ids.exists(&result) {
            result
        } else {
            let mut idx = 1;
            loop {
                let new_id = format!("{}-{}", result, idx);
                if !ids.exists(new_id.as_str()) {
                    return new_id;
                }
                idx += 1;
            }
        }
    }
}

/// A collection of node IDs.
pub struct NodeIds {
    generator: Box<dyn GenerateNodeId>,
    ids: HashSet<String>,
}

impl Default for NodeIds {
    fn default() -> Self {
        NodeIds::new(Box::new(BasicNodeIdGenerator::new()))
    }
}

impl NodeIds {
    /// Creates a new [`NodeIds`] with the given generator.
    pub fn new(generator: Box<dyn GenerateNodeId>) -> Self {
        NodeIds {
            generator,
            ids: HashSet::new(),
        }
    }

    /// Generates a unique ID for the given value and node kind, and stores it.
    pub fn generate(&mut self, value: &str, data: &ast::KindData) -> String {
        let id = self.generator.generate_node_id(value, data, self);
        self.put(&id);
        id
    }

    /// Checks if the given value exists in the IDs.
    pub fn exists(&self, value: &str) -> bool {
        self.ids.contains(value)
    }

    /// Inserts the given value into the IDs.
    pub fn put(&mut self, value: &str) -> bool {
        self.ids.insert(String::from(value))
    }
}

//   }}} NodeIds

//   ContextOptions {{{

/// Options for configuring the [`Context`].
#[derive(Default)]
pub struct ContextOptions {
    pub ids: NodeIds,
    pub size: usize,
}
//   }}} ContextOptions

//   Context {{{

/// A context for parsing operations.
pub struct Context {
    base: context::Context,
    ids: NodeIds,
    link_references: Option<HashMap<String, NodeRef>>,
    block_offset: Option<usize>,
    block_indent: Option<usize>,

    opened_blocks: Vec<BlockPair>,
    delimiters: ParseStack<DelimiterTag>,
    link_labels: ParseStack<LinkLabelTag>,
    link_bottoms: ParseStack<LinkBottomTag>,
}

impl Default for Context {
    fn default() -> Self {
        Context::with_options(ContextOptions::default())
    }
}

impl Context {
    /// Creates a new [`Context`] with default options.
    pub fn new() -> Self {
        Context::default()
    }

    /// Creates a new [`Context`] with the given options.
    pub fn with_options(options: ContextOptions) -> Self {
        Context {
            ids: options.ids,
            base: context::Context::new(),
            block_offset: None,
            block_indent: None,
            link_references: None,
            opened_blocks: Vec::new(),
            delimiters: ParseStack::new(),
            link_labels: ParseStack::new(),
            link_bottoms: ParseStack::new(),
        }
    }

    fn initialize(&mut self, reg: &ContextKeyRegistry) {
        self.base.initialize(reg);
    }

    fn delimiters(&self) -> &ParseStack<DelimiterTag> {
        &self.delimiters
    }

    fn delimiters_mut(&mut self) -> &mut ParseStack<DelimiterTag> {
        &mut self.delimiters
    }

    fn link_labels(&self) -> &ParseStack<LinkLabelTag> {
        &self.link_labels
    }

    fn link_labels_mut(&mut self) -> &mut ParseStack<LinkLabelTag> {
        &mut self.link_labels
    }

    fn link_bottoms(&self) -> &ParseStack<LinkBottomTag> {
        &self.link_bottoms
    }

    fn link_bottoms_mut(&mut self) -> &mut ParseStack<LinkBottomTag> {
        &mut self.link_bottoms
    }

    fn push_link_bottom(&mut self) {
        if let Some(last_delim) = self.last_delimiter() {
            self.link_bottoms_mut().push(last_delim, NODE_REF_UNDEFINED);
        }
    }

    /// Adds a link reference definition to the context.
    pub fn add_link_reference(&mut self, label: impl AsRef<str>, node_ref: NodeRef) {
        if self.link_references.is_none() {
            self.link_references = Some(HashMap::new());
        }
        if let Some(map) = &mut self.link_references {
            let link_ref_key = to_link_reference(label.as_ref().as_bytes());
            let key = unsafe { core::str::from_utf8_unchecked(&link_ref_key) };
            // If there are several matching definitions, the first one takes precedence
            if !map.contains_key(key) {
                map.insert(key.to_string(), node_ref);
            }
        }
    }

    /// Gets a link reference definition by label.
    pub fn link_reference(&self, label: &str) -> Option<NodeRef> {
        if let Some(map) = &self.link_references {
            map.get(label).copied()
        } else {
            None
        }
    }

    /// Gets all link reference definitions.
    pub fn link_references(&self) -> Vec<NodeRef> {
        if let Some(map) = &self.link_references {
            let mut refs = Vec::with_capacity(map.len());
            for v in map.values() {
                refs.push(*v);
            }
            refs
        } else {
            Vec::new()
        }
    }

    /// Gets the last opened delimiter reference.
    pub fn last_delimiter(&self) -> Option<ParseStackElemRef> {
        self.delimiters.top()
    }

    /// Inserts a value into the context.
    pub fn insert<T: AnyValueSpec>(&mut self, key: ContextKey<T>, value: T::Item) {
        self.base.insert(key, value)
    }

    /// Gets a reference to a value from the context.
    pub fn get<T: AnyValueSpec>(&self, key: ContextKey<T>) -> Option<&T::Item> {
        self.base.get(key)
    }

    /// Gets a mutable reference to a value from the context.
    pub fn get_mut<T: AnyValueSpec>(&mut self, key: ContextKey<T>) -> Option<&mut T::Item> {
        self.base.get_mut(key)
    }

    /// Removes a value from the context.
    pub fn remove<T: AnyValueSpec>(&mut self, key: ContextKey<T>) -> Option<T::Item> {
        self.base.remove(key)
    }

    /// Returns an ids.
    #[inline(always)]
    pub fn ids(&self) -> &NodeIds {
        &self.ids
    }

    /// Returns a mutable ids.
    #[inline(always)]
    pub fn ids_mut(&mut self) -> &mut NodeIds {
        &mut self.ids
    }

    /// Returns the last opened block node reference.
    #[inline(always)]
    pub fn last_opened_block(&self) -> Option<NodeRef> {
        self.opened_blocks.last().map(|bp| bp.n)
    }

    /// Sets a first non-space character position on current line.
    /// This value is valid only for [`BlockParser::open`].
    /// None should be set if current line is blank.
    #[inline(always)]
    fn set_block_offset(&mut self, offset: Option<usize>) {
        self.block_offset = offset;
    }

    /// Returns a first non-space character position on current line.
    /// This value is valid only for [`BlockParser::open`].
    /// This returns None if current line is blank.
    #[inline(always)]
    pub fn block_offset(&self) -> Option<usize> {
        self.block_offset
    }

    /// Sets an indent width on current line.
    /// This value is valid only for [`BlockParser::open`].
    /// None should be set if current line is blank.
    #[inline(always)]
    fn set_block_indent(&mut self, indent: Option<usize>) {
        self.block_indent = indent;
    }

    /// Returns an indent width on current line.
    /// This value is valid only for [`BlockParser::open`].
    /// This returns None if current line is blank.
    #[inline(always)]
    pub fn block_indent(&self) -> Option<usize> {
        self.block_indent
    }

    /// Returns whether the parser is currently in a link label.
    #[inline(always)]
    pub fn is_in_link_label(&self) -> bool {
        self.link_labels().bottom().is_some()
    }
}

//   }}} Context

// }}} Context

// ParserOptions & ParserConstructor {{{

/// A trait for parser options.
/// Each parser can define its own options by implementing this trait.
pub trait ParserOptions {}

/// A default implementation of ParserOptions that does nothing.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoParserOptions;

impl ParserOptions for NoParserOptions {}

/// Options for constructing a parser.
pub struct ParserConstructorOptions<T: ParserOptions> {
    options: Options,
    copt: Cell<Option<T>>,
    context_registry: Rc<RefCell<ContextKeyRegistry>>,
}

trait FromParserConstructorOptions<T: ParserOptions> {
    fn from_parser_constructor_options(opt: &ParserConstructorOptions<T>) -> Self;
}

impl<T: ParserOptions> FromParserConstructorOptions<T> for T {
    fn from_parser_constructor_options(opt: &ParserConstructorOptions<T>) -> Self {
        opt.copt.replace(None).unwrap()
    }
}

impl<T: ParserOptions> FromParserConstructorOptions<T> for Options {
    fn from_parser_constructor_options(opt: &ParserConstructorOptions<T>) -> Self {
        opt.options.clone()
    }
}

impl<T: ParserOptions> FromParserConstructorOptions<T> for Rc<RefCell<ContextKeyRegistry>> {
    fn from_parser_constructor_options(opt: &ParserConstructorOptions<T>) -> Self {
        opt.context_registry.clone()
    }
}

/// A trait for constructing parsers with varying arguments.
pub trait ParserConstructor<T, O, R>
where
    O: ParserOptions,
{
    fn call(self, options: &ParserConstructorOptions<O>) -> R;
}

impl<F, O, R> ParserConstructor<(), O, R> for F
// NoArgs
where
    O: ParserOptions,
    F: FnOnce() -> R,
{
    fn call(self, _: &ParserConstructorOptions<O>) -> R {
        self()
    }
}

impl<F, A, O, R> ParserConstructor<(A,), O, R> for F
// 1 Arg
where
    O: ParserOptions,
    F: FnOnce(A) -> R,
    A: FromParserConstructorOptions<O>,
{
    fn call(self, options: &ParserConstructorOptions<O>) -> R {
        let a = A::from_parser_constructor_options(options);
        self(a)
    }
}

impl<F, A, B, O, R> ParserConstructor<(A, B), O, R> for F
// 2 Args
where
    O: ParserOptions,
    F: FnOnce(A, B) -> R,
    A: FromParserConstructorOptions<O>,
    B: FromParserConstructorOptions<O>,
{
    fn call(self, options: &ParserConstructorOptions<O>) -> R {
        let a = A::from_parser_constructor_options(options);
        let b = B::from_parser_constructor_options(options);
        self(a, b)
    }
}

impl<F, A, B, C, O, R> ParserConstructor<(A, B, C), O, R> for F
// 3 Args
where
    O: ParserOptions,
    F: FnOnce(A, B, C) -> R,
    A: FromParserConstructorOptions<O>,
    B: FromParserConstructorOptions<O>,
    C: FromParserConstructorOptions<O>,
{
    fn call(self, options: &ParserConstructorOptions<O>) -> R {
        let a = A::from_parser_constructor_options(options);
        let b = B::from_parser_constructor_options(options);
        let c = C::from_parser_constructor_options(options);
        self(a, b, c)
    }
}

// }}} ParserOptions & ParserConstructor

// Parser {{{

bitflags! {
    /// Represents parser's state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct State: u16 {
        /// Indicates parser may have child blocks.
        const HAS_CHILDREN = 1 << 0;

        /// Indicates parser does not have child blocks.
        const NO_CHILDREN = 1 << 1;

    /// Indicates parser requires that the last node
    /// must be a paragraph and is not converted to other nodes by
    /// ParagraphTransformers.
        const REQUIRE_PARAGRAPH = 1 << 2;
    }
}

/// Options for the parser.
#[derive(Default, Clone)]
pub struct Options {
    /// Enables attributes.
    pub attributes: bool,

    /// Enables auto heading ids.
    pub auto_heading_ids: bool,

    /// If true, default parsers will not be added automatically.
    /// However, if you do not include parsers for IndentedCodeBlock and Paragraph,
    /// the behavior is undefined. If you omit them, you must implement and add:
    ///
    /// - a BlockParser that handles indented lines
    /// - a BlockParser that handles non-indented lines
    pub without_default_parsers: bool,

    /// Options for the AST arena.
    pub arena: ArenaOptions,

    /// If true, a '\' escaped half-space(0x20) will not trigger parsers.
    pub escaped_space: bool,

    /// A custom node ID generator.
    pub id_generator: Option<Rc<dyn GenerateNodeId>>,
}

impl Debug for Options {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Options")
            .field("attributes", &self.attributes)
            .field("auto_heading_ids", &self.auto_heading_ids)
            .field("without_default_parsers", &self.without_default_parsers)
            .field("arena", &self.arena)
            .field("escaped_space", &self.escaped_space)
            .field("id_generator", &"Option<Rc<dyn NodeIdGenerator>>")
            .finish()
    }
}

#[derive(Debug, Copy, Clone)]
struct BlockPair {
    n: NodeRef,
    p: usize,
}

/// A parser for parsing text into an AST.
#[derive(Debug)]
pub struct Parser {
    options: Options,
    context_key_registry: Rc<RefCell<ContextKeyRegistry>>,

    tmp_block_parsers: Option<Vec<Prioritized<AnyBlockParser>>>,
    block_parsers: Vec<AnyBlockParser>,
    block_parser_index: Vec<Option<Vec<usize>>>,
    free_block_parser_index: Vec<usize>,

    tmp_inline_parsers: Option<Vec<Prioritized<AnyInlineParser>>>,
    inline_parsers: Vec<AnyInlineParser>,
    inline_parser_index: Vec<Option<Vec<usize>>>,

    tmp_ast_transformers: Option<Vec<Prioritized<AnyAstTransformer>>>,
    ast_transformers: Vec<AnyAstTransformer>,

    tmp_paragraph_transformers: Option<Vec<Prioritized<AnyParagraphTransformer>>>,
    paragraph_transformers: Vec<AnyParagraphTransformer>,
}

impl Default for Parser {
    fn default() -> Self {
        Parser::with_options(Options::default())
    }
}

/// Priority for setext heading parser.
pub const PRIORITY_SETTEXT_HEADING: u32 = 100;

/// Priority for thematic break parser.
pub const PRIORITY_THEMATIC_BREAK: u32 = 200;

/// Priority for list parser.
pub const PRIORITY_LIST: u32 = 300;

/// Priority for list item parser.
pub const PRIORITY_LIST_ITEM: u32 = 400;

/// Priority for indented code block parser.
pub const PRIORITY_INDENTED_CODE_BLOCK: u32 = 500;

/// Priority for ATX heading parser.
pub const PRIORITY_ATX_HEADING: u32 = 600;

/// Priority for fenced code block parser.
pub const PRIORITY_FENCED_CODE_BLOCK: u32 = 700;

/// Priority for blockquote parser.
pub const PRIORITY_BLOCKQUOTE: u32 = 800;

/// Priority for HTML block parser.
pub const PRIORITY_HTML_BLOCK: u32 = 900;

/// Priority for paragraph parser.
pub const PRIORITY_PARAGRAPH: u32 = 1000;

/// Priority for code span parser.
pub const PRIORITY_CODE_SPAN: u32 = 100;

/// Priority for link parser.
pub const PRIORITY_LINK: u32 = 200;

/// Priority for auto link parser.
pub const PRIORITY_AUTO_LINK: u32 = 300;

/// Priority for raw HTML parser.
pub const PRIORITY_RAW_HTML: u32 = 400;

/// Priority for emphasis parser.
pub const PRIORITY_EMPHASIS: u32 = 500;

struct LineStat {
    pub line_number: usize,
    pub level: usize,
    pub is_blank: bool,
}

fn is_blank_line(line_num: usize, level: usize, stats: &[LineStat]) -> bool {
    let l = stats.len();
    if l == 0 {
        return true;
    }
    let lim = l.saturating_sub(1).saturating_sub(level);
    for i in (0..=lim).rev() {
        let s = &stats[i];
        if s.line_number == line_num && s.level <= level {
            return s.is_blank;
        } else if s.line_number < line_num {
            break;
        }
    }
    false
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    struct LineBreakFlags: u16 {
        const SOFT_LINE_BREAK = 1 << 0;
        const HARD_LINE_BREAK = 1 << 1;
        const VISIBLE = 1 << 2;
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum BlockOpenResult {
    ParagraphContinuation,
    NewBlocksOpened,
    NoBlocksOpened,
}

/// A trait for extending the parser with custom behavior.
pub trait ParserExtension {
    /// Applies the extension to the given parser.
    fn apply(self, parser: &mut Parser);

    /// Chains this extension with another extension.
    fn and(self, other: impl ParserExtension) -> ChainedParserExtension<Self, impl ParserExtension>
    where
        Self: Sized,
    {
        ChainedParserExtension {
            first: self,
            second: other,
        }
    }
}

/// An empty parser extension that does nothing.
#[derive(Debug, Default)]
pub struct EmptyParserExtension;

impl EmptyParserExtension {
    pub fn new() -> Self {
        Self {}
    }
}

impl ParserExtension for EmptyParserExtension {
    fn apply(self, _parser: &mut Parser) {}
}

/// An empty parser extension that does nothing.
pub const NO_EXTENSIONS: EmptyParserExtension = EmptyParserExtension;

/// A parser extension that chains two extensions together.
pub struct ChainedParserExtension<T: ParserExtension, U: ParserExtension> {
    first: T,
    second: U,
}

impl<T: ParserExtension, U: ParserExtension> ParserExtension for ChainedParserExtension<T, U> {
    fn apply(self, parser: &mut Parser) {
        self.first.apply(parser);
        self.second.apply(parser);
    }
}

/// A parser extension defined by a closure.
pub struct ParserExtensionFn<T: FnOnce(&mut Parser)> {
    f: T,
}

impl<T: FnOnce(&mut Parser)> ParserExtensionFn<T> {
    /// Creates a new [`ParserExtensionFn`].
    pub fn new(f: T) -> Self {
        Self { f }
    }
}

impl<T: FnOnce(&mut Parser)> ParserExtension for ParserExtensionFn<T> {
    fn apply(self, parser: &mut Parser) {
        (self.f)(parser);
    }
}

impl<T: FnOnce(&mut Parser)> From<T> for ParserExtensionFn<T> {
    fn from(f: T) -> Self {
        ParserExtensionFn { f }
    }
}

impl Parser {
    /// Creates a new [`Parser`] with default options.
    pub fn new() -> Self {
        Self::with_options(Options::default())
    }

    /// Creates a new [`Parser`] with the given options.
    pub fn with_options(options: Options) -> Self {
        Self::with_extensions(options, EmptyParserExtension::new())
    }

    /// Creates a new [`Parser`] with the given options and extensions.
    ///
    /// - [`add_block_parser`] can only be called within extensions.
    /// - [`add_inline_parser`] can only be called within extensions.
    /// - [`add_paragraph_transformer`] can only be called within extensions.
    /// - [`add_ast_transformer`] can only be called within extensions.
    ///
    /// [`add_block_parser`]: Self::add_block_parser
    /// [`add_inline_parser`]: Self::add_inline_parser
    /// [`add_paragraph_transformer`]: Self::add_paragraph_transformer
    /// [`add_ast_transformer`]: Self::add_ast_transformer
    pub fn with_extensions(options: Options, ext: impl ParserExtension) -> Self {
        let mut p = Parser {
            options,
            context_key_registry: Rc::new(RefCell::new(ContextKeyRegistry::default())),

            tmp_block_parsers: Some(Vec::new()),
            block_parsers: Vec::new(),
            block_parser_index: iter::repeat_n(0, 256).map(|_| None).collect(),
            free_block_parser_index: Vec::new(),

            tmp_inline_parsers: Some(Vec::new()),
            inline_parsers: Vec::new(),
            inline_parser_index: iter::repeat_n(0, 256).map(|_| None).collect(),

            tmp_ast_transformers: None,
            ast_transformers: Vec::new(),

            tmp_paragraph_transformers: None,
            paragraph_transformers: Vec::new(),
        };
        if !p.options.without_default_parsers {
            p.add_default_block_parsers();
            p.add_default_inline_parsers();
            p.add_default_paragraph_transformers();
        }
        ext.apply(&mut p);
        p.init();
        p
    }

    fn add_default_block_parsers(&mut self) {
        self.add_block_parser(ParagraphParser::new, NoParserOptions, PRIORITY_PARAGRAPH);
        self.add_block_parser(BlockquoteParser::new, NoParserOptions, PRIORITY_BLOCKQUOTE);
        self.add_block_parser(
            IndentedCodeBlockParser::new,
            NoParserOptions,
            PRIORITY_INDENTED_CODE_BLOCK,
        );
        self.add_block_parser(
            FencedCodeBlockParser::new,
            NoParserOptions,
            PRIORITY_INDENTED_CODE_BLOCK,
        );
        self.add_block_parser(AtxHeadingParser::new, NoParserOptions, PRIORITY_ATX_HEADING);
        self.add_block_parser(
            SetextHeadingParser::new,
            NoParserOptions,
            PRIORITY_SETTEXT_HEADING,
        );
        self.add_block_parser(
            ThematicBreakParser::new,
            NoParserOptions,
            PRIORITY_THEMATIC_BREAK,
        );
        self.add_block_parser(ListParser::new, NoParserOptions, PRIORITY_LIST);
        self.add_block_parser(ListItemParser::new, NoParserOptions, PRIORITY_LIST_ITEM);
        self.add_block_parser(HtmlBlockParser::new, NoParserOptions, PRIORITY_HTML_BLOCK);
    }

    fn add_default_inline_parsers(&mut self) {
        self.add_inline_parser(CodeSpanParser::new, NoParserOptions, PRIORITY_CODE_SPAN);
        self.add_inline_parser(RawHtmlParser::new, NoParserOptions, PRIORITY_RAW_HTML);
        self.add_inline_parser(EmphasisParser::new, NoParserOptions, PRIORITY_EMPHASIS);
        self.add_inline_parser(LinkParser::new, NoParserOptions, PRIORITY_LINK);
        self.add_inline_parser(AutoLinkParser::new, NoParserOptions, PRIORITY_AUTO_LINK);
    }

    fn add_default_paragraph_transformers(&mut self) {
        self.add_paragraph_transformer(
            LinkReferenceParagraphTransformer::new,
            NoParserOptions,
            100,
        );
    }

    fn init(&mut self) {
        if self.tmp_block_parsers.is_some() {
            if let Some(mut tmp_block_parsers) = self.tmp_block_parsers.take() {
                tmp_block_parsers.sort();
                for mut bp in tmp_block_parsers.drain(..) {
                    let item = bp.take();
                    let idx = self.block_parsers.len();
                    let trigger = item.trigger();
                    if trigger.is_empty() {
                        self.free_block_parser_index.push(idx);
                    } else {
                        for &c in trigger {
                            let ch = c as usize;
                            if let Some(vec) = &mut self.block_parser_index[ch] {
                                vec.push(idx);
                            } else {
                                self.block_parser_index[ch] = Some(vec![idx]);
                            }
                        }
                    }
                    self.block_parsers.push(item);
                }
                for i in 0..256 {
                    if let Some(vec) = &mut self.block_parser_index[i] {
                        vec.extend(&self.free_block_parser_index);
                    }
                }
            }
        }
        if self.tmp_inline_parsers.is_some() {
            if let Some(mut tmp_inline_parsers) = self.tmp_inline_parsers.take() {
                tmp_inline_parsers.sort();
                for mut bp in tmp_inline_parsers.drain(..) {
                    let item = bp.take();
                    let idx = self.inline_parsers.len();
                    let trigger = item.trigger();
                    if !trigger.is_empty() {
                        for &c in trigger {
                            let ch = c as usize;
                            if let Some(vec) = &mut self.inline_parser_index[ch] {
                                vec.push(idx);
                            } else {
                                self.inline_parser_index[ch] = Some(vec![idx]);
                            }
                        }
                    }
                    self.inline_parsers.push(item);
                }
            }
        }
        if let Some(mut tmp_ast_transformers) = self.tmp_ast_transformers.take() {
            tmp_ast_transformers.sort();
            for mut transformer in tmp_ast_transformers.drain(..) {
                let item = transformer.take();
                self.ast_transformers.push(item);
            }
        }
        if let Some(mut tmp_paragraph_transformers) = self.tmp_paragraph_transformers.take() {
            tmp_paragraph_transformers.sort();
            for mut transformer in tmp_paragraph_transformers.drain(..) {
                let item = transformer.take();
                self.paragraph_transformers.push(item);
            }
        }
    }

    /// Adds a [`BlockParser`] to the parser with the given options and priority.
    pub fn add_block_parser<T, O, R, F>(&mut self, f: F, copt: O, priority: u32)
    where
        O: ParserOptions,
        F: ParserConstructor<T, O, R>,
        R: Into<AnyBlockParser>,
    {
        let bp = f.call(&ParserConstructorOptions {
            options: self.options.clone(),
            copt: Cell::new(Some(copt)),
            context_registry: self.context_key_registry.clone(),
        });
        if let Some(tmp_block_parsers) = &mut self.tmp_block_parsers {
            tmp_block_parsers.push(Prioritized::new(bp.into(), priority));
        }
    }

    /// Adds an [`InlineParser`] to the parser with the given options and priority.
    pub fn add_inline_parser<T, O, R, F>(&mut self, f: F, copt: O, priority: u32)
    where
        O: ParserOptions,
        F: ParserConstructor<T, O, R>,
        R: Into<AnyInlineParser>,
    {
        let ip = f.call(&ParserConstructorOptions {
            options: self.options.clone(),
            copt: Cell::new(Some(copt)),
            context_registry: self.context_key_registry.clone(),
        });
        if let Some(tmp_inline_parsers) = &mut self.tmp_inline_parsers {
            tmp_inline_parsers.push(Prioritized::new(ip.into(), priority));
        }
    }

    /// Add an [`AstTransformer`] to the parser with the given priority.
    pub fn add_ast_transformer<T, O, R, F>(&mut self, f: F, copt: O, priority: u32)
    where
        O: ParserOptions,
        F: ParserConstructor<T, O, R>,
        R: Into<AnyAstTransformer>,
    {
        let transformer = f.call(&ParserConstructorOptions {
            options: self.options.clone(),
            copt: Cell::new(Some(copt)),
            context_registry: self.context_key_registry.clone(),
        });
        if self.tmp_ast_transformers.is_none() {
            self.tmp_ast_transformers = Some(Vec::new());
        }
        if let Some(tmp_ast_transformers) = &mut self.tmp_ast_transformers {
            tmp_ast_transformers.push(Prioritized::new(transformer.into(), priority));
        }
    }

    /// Add a [`ParagraphTransformer`] to the parser with the given priority.
    pub fn add_paragraph_transformer<T, O, R, F>(&mut self, f: F, copt: O, priority: u32)
    where
        O: ParserOptions,
        F: ParserConstructor<T, O, R>,
        R: Into<AnyParagraphTransformer>,
    {
        let transformer = f.call(&ParserConstructorOptions {
            options: self.options.clone(),
            copt: Cell::new(Some(copt)),
            context_registry: self.context_key_registry.clone(),
        });
        if self.tmp_paragraph_transformers.is_none() {
            self.tmp_paragraph_transformers = Some(Vec::new());
        }
        if let Some(tmp_paragraph_transformers) = &mut self.tmp_paragraph_transformers {
            tmp_paragraph_transformers.push(Prioritized::new(transformer.into(), priority));
        }
    }

    /// Parses the input from the given reader and returns the resulting AST arena and root
    /// document node.
    pub fn parse(&self, reader: &mut text::BasicReader) -> (Arena, NodeRef) {
        let mut arena = Arena::with_options(self.options.arena);
        let context = &mut Context::new();
        let reg = self.context_key_registry.borrow();
        context.initialize(&reg);

        let doc = arena.new_node(ast::Document::new());
        self.parse_blocks(&mut arena, doc, reader, context);

        let mut blocks = vec![];
        ast::walk(&arena, doc, &mut |_: &Arena,
                                     node_ref: NodeRef,
                                     entering: bool|
         -> Result<ast::WalkStatus> {
            if entering {
                blocks.push(node_ref);
                return Ok(ast::WalkStatus::Continue);
            }
            Ok(ast::WalkStatus::SkipChildren)
        })
        .map_err(|e| match e {
            CallbackError::Internal(err) => err,
            _ => panic!("should not happen"),
        })
        .expect("walk failed");

        for &block in &blocks {
            self.parse_block(&mut arena, block, reader, context);
        }

        for transformer in &self.ast_transformers {
            transformer.transform(&mut arena, doc, reader, context);
        }

        #[cfg(feature = "pp-ast")]
        {
            use crate::ast::pretty_print;
            let mut out = String::new();
            pretty_print(&mut out, &arena, doc, reader.source()).unwrap();
            println!("{}", out);
        }

        (arena, doc)
    }

    fn open_blocks(
        &self,
        arena: &mut Arena,
        p: NodeRef,
        blank_line: bool,
        reader: &mut text::BasicReader,
        pc: &mut Context,
    ) -> BlockOpenResult {
        let mut parent = p;
        let mut result = BlockOpenResult::NoBlocksOpened;
        let mut continuable = false;
        let last_opened_block = pc.opened_blocks.last().cloned();
        if let Some(b) = last_opened_block {
            continuable = matches_kind!(arena, b.n, Paragraph);
        }
        'try_bps: while let Some((line, _)) = reader.peek_line_bytes() {
            if line.is_empty() || line[0] == b'\n' {
                break;
            }

            let (w, pos) = util::indent_width(&line, reader.line_offset());
            if w >= line.len() {
                pc.set_block_indent(None);
                pc.set_block_offset(None);
            } else {
                pc.set_block_indent(Some(w));
                pc.set_block_offset(Some(pos));
            }

            let mut bps = &self.free_block_parser_index;
            if pos < line.len() {
                bps = self.block_parser_index[line[pos] as usize]
                    .as_ref()
                    .unwrap_or(bps);
            }

            for &pidx in bps {
                let bp = &self.block_parsers[pidx];
                if continuable
                    && result == BlockOpenResult::NoBlocksOpened
                    && !bp.can_interrupt_paragraph()
                {
                    continue;
                }
                if w > 3 && !bp.can_accept_indented_line() {
                    continue;
                }

                let (_, block_pos) = reader.position();
                let last_block = pc.opened_blocks.last().cloned();
                if let Some((node_ref, state)) = bp.open(arena, parent, reader, pc) {
                    if !arena[node_ref].has_pos() {
                        arena[node_ref].set_pos(block_pos.start() + pc.block_offset().unwrap_or(0));
                    }

                    // Parser requires last node to be a paragraph.
                    // With table extension:
                    //
                    //     0
                    //     -:
                    //     -
                    //
                    // '-' on 3rd line seems a Setext heading because 1st and 2nd lines
                    // are being paragraph when the Settext heading parser tries to parse the 3rd
                    // line.
                    // But 1st line and 2nd line are a table. Thus this paragraph will be transformed
                    // by a paragraph transformer. So this text should be converted to a table and
                    // an empty list.
                    if state.contains(State::REQUIRE_PARAGRAPH) {
                        if let Some(last_block) = last_block {
                            if arena[parent]
                                .last_child()
                                .is_some_and(|r| r == last_block.n)
                            {
                                self.block_parsers[last_block.p].close(
                                    arena,
                                    last_block.n,
                                    reader,
                                    pc,
                                );
                                pc.opened_blocks.pop();
                                if self.transform_paragraphs(arena, last_block.n, reader, pc) {
                                    // Paragraph has been transformed.
                                    // So this parser is considered as failing.
                                    continuable = false;
                                    continue 'try_bps;
                                }
                            }
                        }
                    }
                    {
                        let block = as_type_data_mut!(arena, node_ref, Block);
                        block.set_blank_previous_line(blank_line);
                    };

                    if matches!(last_block, Some(l) if arena[l.n].parent().is_none())
                        && !pc.opened_blocks.is_empty()
                    {
                        let last_pos = pc.opened_blocks.len() - 1;
                        self.close_blocks(arena, last_pos, last_pos, reader, pc);
                    }
                    parent.append_child_fast(arena, node_ref);
                    result = BlockOpenResult::NewBlocksOpened;
                    pc.opened_blocks.push(BlockPair {
                        n: node_ref,
                        p: pidx,
                    });

                    if state.contains(State::HAS_CHILDREN) {
                        parent = node_ref;
                        continue 'try_bps; // try child block
                    }
                    break; // no children, can not open more blocks on this line
                }
            }
            break;
        }

        if result == BlockOpenResult::NoBlocksOpened && continuable {
            let state = match last_opened_block {
                Some(b) => self.block_parsers[b.p].cont(arena, b.n, reader, pc),
                None => None,
            };
            if state.is_some() {
                result = BlockOpenResult::ParagraphContinuation;
            }
        }
        result
    }

    fn parse_blocks(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) {
        ctx.opened_blocks.clear();

        let mut blank_lines: Vec<LineStat> = Vec::with_capacity(128);

        // process blocks separated by blank lines
        loop {
            if reader.skip_blank_lines().is_none() {
                break;
            }

            // first, we try to open blocks
            if self.open_blocks(arena, parent_ref, true, reader, ctx)
                != BlockOpenResult::NewBlocksOpened
            {
                return;
            }
            reader.advance_line();
            blank_lines.clear();
            // process opened blocks line by line
            loop {
                if ctx.opened_blocks.is_empty() {
                    break;
                }
                let l = ctx.opened_blocks.len();
                let mut last_index = l - 1;
                for i in 0..l {
                    let be = ctx.opened_blocks[i];
                    let Some((line, _)) = reader.peek_line_bytes() else {
                        self.close_blocks(arena, last_index, 0, reader, ctx);
                        reader.advance_line();
                        break;
                    };
                    let (line_num, _) = reader.position();
                    blank_lines.push(LineStat {
                        line_number: line_num,
                        level: i,
                        is_blank: is_blank(line.as_ref()),
                    });
                    // If node is a paragraph, open_blocks determines whether it is continuable.
                    // So we do not process paragraphs here.
                    if !matches_kind!(arena, be.n, Paragraph) {
                        let state = self.block_parsers[be.p].cont(arena, be.n, reader, ctx);
                        if let Some(state) = state {
                            // When current node is a container block and has no children,
                            // we try to open new child nodes
                            if state.contains(State::HAS_CHILDREN) && i == last_index {
                                let is_blank = is_blank_line(line_num - 1, i + 1, &blank_lines);
                                self.open_blocks(arena, be.n, is_blank, reader, ctx);
                                break;
                            }
                            continue;
                        }
                    }
                    // current node may be closed or lazy continuation
                    let is_blank = is_blank_line(line_num - 1, i, &blank_lines);
                    let mut this_parent_ref = parent_ref;
                    if i != 0 {
                        this_parent_ref = ctx.opened_blocks[i - 1].n;
                    }
                    let last_node_ref = ctx.opened_blocks[last_index].n;
                    let result = self.open_blocks(arena, this_parent_ref, is_blank, reader, ctx);
                    if result != BlockOpenResult::ParagraphContinuation {
                        // lastNode is a paragraph and was transformed by the paragraph
                        // transformers.
                        if last_index >= ctx.opened_blocks.len()
                            || ctx.opened_blocks[last_index].n != last_node_ref
                        {
                            if last_index != 0 {
                                last_index -= 1;
                                self.close_blocks(arena, last_index, i, reader, ctx);
                            }
                        } else {
                            self.close_blocks(arena, last_index, i, reader, ctx);
                        }
                    }
                    break;
                }

                reader.advance_line();
            }
        }
    }

    fn parse_block(
        &self,
        arena: &mut Arena,
        block_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) {
        let bd = as_type_data_mut!(arena, block_ref, Block);
        if bd.source().is_empty() {
            return;
        }

        let lines = bd.take_source();
        let mut escaped = false;
        let mut block_reader = text::BlockReader::new(reader.source(), &lines);

        'try_lines: while let Some((line, _)) = block_reader.peek_line_bytes() {
            let mut line_len = line.len();
            let mut flags = LineBreakFlags::empty();
            let has_nl = line.ends_with(b"\n");
            if ((line_len >= 3 && line[line_len - 2] == b'\\' && line[line_len - 3] != b'\\')
                || (line_len == 2 && line[line_len - 2] == b'\\'))
                && has_nl
            {
                line_len -= 2;
                flags |= LineBreakFlags::HARD_LINE_BREAK | LineBreakFlags::VISIBLE;
            } else if ((line_len >= 4
                && line[line_len - 3] == b'\\'
                && line[line_len - 2] == b'\r'
                && line[line_len - 4] != b'\\')
                || (line_len == 3 && line[line_len - 3] == b'\\' && line[line_len - 2] == b'\r'))
                && has_nl
            {
                line_len -= 3;
                flags |= LineBreakFlags::HARD_LINE_BREAK | LineBreakFlags::VISIBLE;
            } else if line_len >= 3
                && line[line_len - 3] == b' '
                && line[line_len - 2] == b' '
                && has_nl
            {
                line_len -= 3;
                flags |= LineBreakFlags::HARD_LINE_BREAK;
            } else if line_len >= 4
                && line[line_len - 4] == b' '
                && line[line_len - 3] == b' '
                && line[line_len - 2] == b'\r'
                && has_nl
            {
                line_len -= 4;
                flags |= LineBreakFlags::HARD_LINE_BREAK;
            } else if has_nl {
                // If the line ends with a newline character, but it is not a hardlineBreak, then it is a softLinebreak
                // If the line ends with a hardlineBreak, then it cannot end with a softLinebreak
                // See https://spec.commonmark.org/0.30/#soft-line-breaks
                flags |= LineBreakFlags::SOFT_LINE_BREAK;
            }

            let (l, mut start_position) = block_reader.position();
            let mut n = 0;
            for i in 0..line_len {
                let c = line[i];
                if c == b'\n' {
                    break;
                }
                let is_space = is_space(c) && c != b'\r' && c != b'\n';
                let is_punct = is_punct(c);
                if (is_punct && !escaped)
                    || (is_space && !(escaped && self.options.escaped_space))
                    || i == 0
                {
                    let mut parser_char = c;
                    if is_space || (i == 0 && !is_punct) {
                        parser_char = b' ';
                    }
                    let ips_idx = &self.inline_parser_index[parser_char as usize];
                    if let Some(ips_idx) = ips_idx {
                        block_reader.advance(n);
                        n = 0;
                        let (saved_line, saved_position) = block_reader.position();
                        if i != 0 {
                            let (_, current_position) = block_reader.position();
                            if start_position.stop() == current_position.stop() {
                                let bw = start_position.between(current_position);
                                block_ref.merge_or_append_text(arena, bw.into());
                            }
                            let (_, sp) = block_reader.position();
                            start_position = sp;
                        }
                        let mut inline_node_ref_opt: Option<NodeRef> = None;
                        for &ip_idx in ips_idx {
                            let ip = &self.inline_parsers[ip_idx];
                            inline_node_ref_opt =
                                ip.parse(arena, block_ref, &mut block_reader, ctx);
                            if let Some(inline_node_ref) = inline_node_ref_opt {
                                if !arena[inline_node_ref].has_pos() {
                                    arena[inline_node_ref].set_pos(saved_position.start());
                                }
                                break;
                            }
                            block_reader.set_position(saved_line, saved_position);
                        }
                        if let Some(inline_node_ref) = inline_node_ref_opt {
                            block_ref.append_child_fast(arena, inline_node_ref);
                            continue 'try_lines;
                        }
                    }
                }
                if escaped {
                    escaped = false;
                    n += 1;
                    continue;
                }

                if c == b'\\' {
                    escaped = true;
                    n += 1;
                    continue;
                }

                escaped = false;
                n += 1;
            }

            if n != 0 {
                block_reader.advance(n);
            }
            let (current_l, current_position) = block_reader.position();
            if l != current_l {
                continue;
            }
            let diff = start_position.between(current_position);
            let mut text_node =
                if flags.contains(LineBreakFlags::HARD_LINE_BREAK | LineBreakFlags::VISIBLE) {
                    Text::new(diff)
                } else {
                    Text::new(diff.trim_right_space(reader.source()))
                };
            if flags.contains(LineBreakFlags::SOFT_LINE_BREAK) {
                text_node.add_qualifiers(TextQualifier::SOFT_LINE_BREAK);
            }
            if flags.contains(LineBreakFlags::HARD_LINE_BREAK) {
                text_node.add_qualifiers(TextQualifier::HARD_LINE_BREAK);
            }
            let text_node_ref = arena.new_node(text_node);
            block_ref.append_child_fast(arena, text_node_ref);
            block_reader.advance_line();
        }

        process_delimiters(arena, None, ctx);

        for ip in &self.inline_parsers {
            ip.close_block(arena, block_ref, &mut block_reader, ctx);
        }

        as_type_data_mut!(arena, block_ref, Block).put_back_source(lines);
    }

    fn transform_paragraphs(
        &self,
        arena: &mut Arena,
        paragraph_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> bool {
        for transformer in &self.paragraph_transformers {
            transformer.transform(arena, paragraph_ref, reader, ctx);
            if arena.get(paragraph_ref).is_none() {
                return true;
            }
        }
        false
    }

    fn close_blocks(
        &self,
        arena: &mut Arena,
        from: usize,
        to: usize,
        reader: &mut text::BasicReader,
        pc: &mut Context,
    ) {
        for i in (to..=from).rev() {
            let node_ref = pc.opened_blocks[i].n;
            if matches_kind!(arena, pc.opened_blocks[i].n, Paragraph)
                && arena[node_ref].parent().is_some()
            {
                self.transform_paragraphs(arena, node_ref, reader, pc);
            }
            if arena.get(node_ref).is_some() {
                // closes only if node has not been transformed
                self.block_parsers[pc.opened_blocks[i].p].close(arena, node_ref, reader, pc);
            }
        }
        if from == pc.opened_blocks.len() - 1 {
            pc.opened_blocks.truncate(to);
        } else {
            pc.opened_blocks.drain(to..=from).for_each(drop);
        }
    }
}

// }}} Parser

// BlockParser {{{

/// An enum of all block parsers.
#[derive(Debug)]
#[non_exhaustive]
pub enum AnyBlockParser {
    Paragraph(ParagraphParser),
    Blockquote(BlockquoteParser),
    AtxHeading(AtxHeadingParser),
    SetextHeading(SetextHeadingParser),
    ThematicBreak(ThematicBreakParser),
    List(ListParser),
    ListItem(ListItemParser),
    HtmlBlock(HtmlBlockParser),
    IndentedCodeBlock(IndentedCodeBlockParser),
    FencedCodeBlock(FencedCodeBlockParser),

    Extension(Box<dyn BlockParser>),
}

impl BlockParser for AnyBlockParser {
    fn trigger(&self) -> &[u8] {
        match self {
            AnyBlockParser::Paragraph(p) => p.trigger(),
            AnyBlockParser::Blockquote(p) => p.trigger(),
            AnyBlockParser::AtxHeading(p) => p.trigger(),
            AnyBlockParser::SetextHeading(p) => p.trigger(),
            AnyBlockParser::ThematicBreak(p) => p.trigger(),
            AnyBlockParser::List(p) => p.trigger(),
            AnyBlockParser::ListItem(p) => p.trigger(),
            AnyBlockParser::HtmlBlock(p) => p.trigger(),
            AnyBlockParser::IndentedCodeBlock(p) => p.trigger(),
            AnyBlockParser::FencedCodeBlock(p) => p.trigger(),
            AnyBlockParser::Extension(p) => p.trigger(),
        }
    }

    fn open(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        match self {
            AnyBlockParser::Paragraph(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::Blockquote(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::AtxHeading(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::SetextHeading(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::ThematicBreak(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::List(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::ListItem(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::HtmlBlock(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::IndentedCodeBlock(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::FencedCodeBlock(p) => p.open(arena, parent_ref, reader, ctx),
            AnyBlockParser::Extension(p) => p.open(arena, parent_ref, reader, ctx),
        }
    }

    fn cont(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<State> {
        match self {
            AnyBlockParser::Paragraph(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::Blockquote(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::AtxHeading(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::SetextHeading(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::ThematicBreak(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::List(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::ListItem(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::HtmlBlock(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::IndentedCodeBlock(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::FencedCodeBlock(p) => p.cont(arena, node_ref, reader, ctx),
            AnyBlockParser::Extension(p) => p.cont(arena, node_ref, reader, ctx),
        }
    }

    fn close(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) {
        match self {
            AnyBlockParser::Paragraph(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::Blockquote(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::AtxHeading(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::SetextHeading(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::ThematicBreak(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::List(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::ListItem(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::HtmlBlock(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::IndentedCodeBlock(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::FencedCodeBlock(p) => p.close(arena, node_ref, reader, ctx),
            AnyBlockParser::Extension(p) => p.close(arena, node_ref, reader, ctx),
        }
    }

    fn can_interrupt_paragraph(&self) -> bool {
        match self {
            AnyBlockParser::Paragraph(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::Blockquote(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::AtxHeading(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::SetextHeading(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::ThematicBreak(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::List(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::ListItem(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::HtmlBlock(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::IndentedCodeBlock(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::FencedCodeBlock(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::Extension(p) => p.can_interrupt_paragraph(),
        }
    }

    fn can_accept_indented_line(&self) -> bool {
        match self {
            AnyBlockParser::Paragraph(p) => p.can_accept_indented_line(),
            AnyBlockParser::Blockquote(p) => p.can_accept_indented_line(),
            AnyBlockParser::AtxHeading(p) => p.can_accept_indented_line(),
            AnyBlockParser::SetextHeading(p) => p.can_accept_indented_line(),
            AnyBlockParser::ThematicBreak(p) => p.can_accept_indented_line(),
            AnyBlockParser::List(p) => p.can_accept_indented_line(),
            AnyBlockParser::ListItem(p) => p.can_accept_indented_line(),
            AnyBlockParser::HtmlBlock(p) => p.can_interrupt_paragraph(),
            AnyBlockParser::IndentedCodeBlock(p) => p.can_accept_indented_line(),
            AnyBlockParser::FencedCodeBlock(p) => p.can_accept_indented_line(),
            AnyBlockParser::Extension(p) => p.can_accept_indented_line(),
        }
    }
}

impl From<ParagraphParser> for AnyBlockParser {
    fn from(p: ParagraphParser) -> Self {
        AnyBlockParser::Paragraph(p)
    }
}

impl From<BlockquoteParser> for AnyBlockParser {
    fn from(p: BlockquoteParser) -> Self {
        AnyBlockParser::Blockquote(p)
    }
}

impl From<AtxHeadingParser> for AnyBlockParser {
    fn from(p: AtxHeadingParser) -> Self {
        AnyBlockParser::AtxHeading(p)
    }
}

impl From<SetextHeadingParser> for AnyBlockParser {
    fn from(p: SetextHeadingParser) -> Self {
        AnyBlockParser::SetextHeading(p)
    }
}

impl From<ThematicBreakParser> for AnyBlockParser {
    fn from(p: ThematicBreakParser) -> Self {
        AnyBlockParser::ThematicBreak(p)
    }
}

impl From<ListParser> for AnyBlockParser {
    fn from(p: ListParser) -> Self {
        AnyBlockParser::List(p)
    }
}

impl From<ListItemParser> for AnyBlockParser {
    fn from(p: ListItemParser) -> Self {
        AnyBlockParser::ListItem(p)
    }
}

impl From<HtmlBlockParser> for AnyBlockParser {
    fn from(p: HtmlBlockParser) -> Self {
        AnyBlockParser::HtmlBlock(p)
    }
}

impl From<IndentedCodeBlockParser> for AnyBlockParser {
    fn from(p: IndentedCodeBlockParser) -> Self {
        AnyBlockParser::IndentedCodeBlock(p)
    }
}

impl From<FencedCodeBlockParser> for AnyBlockParser {
    fn from(p: FencedCodeBlockParser) -> Self {
        AnyBlockParser::FencedCodeBlock(p)
    }
}

impl From<Box<dyn BlockParser>> for AnyBlockParser {
    fn from(p: Box<dyn BlockParser>) -> Self {
        AnyBlockParser::Extension(p)
    }
}

/// A trait that parses a block level element like Paragraph, List,
/// Blockquote etc.
pub trait BlockParser: Debug {
    /// Returns a list of characters that triggers Parse method of
    /// this parser.
    /// If `trigger` returns a empty slice, `open` will be called with any lines.
    fn trigger(&self) -> &[u8];

    /// Parses the current line and returns a result of parsing.
    ///
    /// `open` must not parse beyond the current line.
    /// If `open` has been able to parse the current line, `open` must advance a reader
    /// position by consumed byte length.
    ///
    /// If `open` has not been able to parse the current line, Open should returns
    /// `(None, State::NO_CHILDREN)`. If `open` has been able to parse the current line,`open`
    /// should returns a new Block node ref and returns `State::HAS_CHILDREN` or
    /// `State::NO_CHILDREN`.
    fn open(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<(NodeRef, State)>;

    /// Parses the current line and returns a result of parsing.
    ///
    /// `cont` must not parse beyond the current line.
    /// If `cont` has been able to parse the current line, `cont` must advance
    /// a reader position by consumed byte length.
    ///
    /// If `cont` has not been able to parse the current line, `cont` should
    /// returns `None`. If `cont` has been able to parse the current line,
    /// `cont` should returns `State::NO_CHILDREN` or
    /// `State::HAS_CHILDREN` .
    fn cont(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<State>;

    /// `close` will be called when the parser returns `State::CLOSE`.
    fn close(
        &self,
        _arena: &mut Arena,
        _node_ref: NodeRef,
        _reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) {
    }

    /// Returns true if the parser can interrupt paragraphs,
    /// otherwise false.
    fn can_interrupt_paragraph(&self) -> bool {
        false
    }

    /// Returns true if the parser can open new node when
    /// the given line is being indented more than 3 spaces.
    fn can_accept_indented_line(&self) -> bool {
        false
    }
}

// }}} BlockParser

// InlineParser {{{

/// An enum of all inline parsers.
#[derive(Debug)]
#[non_exhaustive]
pub enum AnyInlineParser {
    CodeSpan(CodeSpanParser),
    RawHtml(RawHtmlParser),
    Emphasis(EmphasisParser),
    Link(LinkParser),
    AutoLink(AutoLinkParser),

    Linkify(LinkifyParser),
    Strikethrough(StrikethroughParser),

    Extension(Box<dyn InlineParser>),
}

impl InlineParser for AnyInlineParser {
    fn trigger(&self) -> &[u8] {
        match self {
            AnyInlineParser::CodeSpan(p) => p.trigger(),
            AnyInlineParser::RawHtml(p) => p.trigger(),
            AnyInlineParser::Emphasis(p) => p.trigger(),
            AnyInlineParser::Link(p) => p.trigger(),
            AnyInlineParser::AutoLink(p) => p.trigger(),

            AnyInlineParser::Linkify(p) => p.trigger(),
            AnyInlineParser::Strikethrough(p) => p.trigger(),

            AnyInlineParser::Extension(ext) => ext.trigger(),
        }
    }

    fn parse(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut Context,
    ) -> Option<NodeRef> {
        match self {
            AnyInlineParser::CodeSpan(p) => p.parse(arena, parent_ref, reader, ctx),
            AnyInlineParser::RawHtml(p) => p.parse(arena, parent_ref, reader, ctx),
            AnyInlineParser::Emphasis(p) => p.parse(arena, parent_ref, reader, ctx),
            AnyInlineParser::Link(p) => p.parse(arena, parent_ref, reader, ctx),
            AnyInlineParser::AutoLink(p) => p.parse(arena, parent_ref, reader, ctx),

            AnyInlineParser::Linkify(p) => p.parse(arena, parent_ref, reader, ctx),
            AnyInlineParser::Strikethrough(p) => p.parse(arena, parent_ref, reader, ctx),

            AnyInlineParser::Extension(ext) => ext.parse(arena, parent_ref, reader, ctx),
        }
    }

    #[inline(always)]
    fn close_block(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut Context,
    ) {
        // most parsers do not need this. To optimize, we inline this method.
        match self {
            AnyInlineParser::CodeSpan(_) => (),
            AnyInlineParser::RawHtml(_) => (),
            AnyInlineParser::Emphasis(_) => (),
            AnyInlineParser::AutoLink(_) => (),
            AnyInlineParser::Link(p) => p.close_block(arena, node_ref, reader, ctx),

            AnyInlineParser::Linkify(p) => p.close_block(arena, node_ref, reader, ctx),
            AnyInlineParser::Strikethrough(_) => (),

            AnyInlineParser::Extension(ext) => ext.close_block(arena, node_ref, reader, ctx),
        }
    }
}

impl From<CodeSpanParser> for AnyInlineParser {
    fn from(p: CodeSpanParser) -> Self {
        AnyInlineParser::CodeSpan(p)
    }
}

impl From<RawHtmlParser> for AnyInlineParser {
    fn from(p: RawHtmlParser) -> Self {
        AnyInlineParser::RawHtml(p)
    }
}

impl From<EmphasisParser> for AnyInlineParser {
    fn from(p: EmphasisParser) -> Self {
        AnyInlineParser::Emphasis(p)
    }
}

impl From<LinkParser> for AnyInlineParser {
    fn from(p: LinkParser) -> Self {
        AnyInlineParser::Link(p)
    }
}

impl From<AutoLinkParser> for AnyInlineParser {
    fn from(p: AutoLinkParser) -> Self {
        AnyInlineParser::AutoLink(p)
    }
}

impl From<LinkifyParser> for AnyInlineParser {
    fn from(p: LinkifyParser) -> Self {
        AnyInlineParser::Linkify(p)
    }
}

impl From<StrikethroughParser> for AnyInlineParser {
    fn from(p: StrikethroughParser) -> Self {
        AnyInlineParser::Strikethrough(p)
    }
}

impl From<Box<dyn InlineParser>> for AnyInlineParser {
    fn from(ext: Box<dyn InlineParser>) -> Self {
        AnyInlineParser::Extension(ext)
    }
}

/// A trait that parses an inline level element like CodeSpan, Link etc.
pub trait InlineParser: Debug {
    /// Returns a list of characters that triggers Parse method of
    /// this parser.
    /// Trigger characters must be a punctuation or a halfspace.
    /// Halfspaces triggers this parser when character is any spaces characters or
    /// a head of line
    fn trigger(&self) -> &[u8];

    /// Parses the given block into an inline node.
    ///
    /// Parse can parse beyond the current line.
    /// If Parse has been able to parse the current line, it must advance a reader
    /// position by consumed byte length.
    fn parse(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        ctx: &mut Context,
    ) -> Option<NodeRef>;

    /// close_block will be called when a block is closed.
    fn close_block(
        &self,
        _arena: &mut Arena,
        _node_ref: NodeRef,
        _reader: &mut text::BlockReader,
        _ctx: &mut Context,
    ) {
    }
}

// }}} InlineParser

// AstTransformer {{{

/// A trait that transforms AST nodes.
pub trait AstTransformer: Debug {
    /// Transforms the given Document node.
    fn transform(
        &self,
        arena: &mut Arena,
        doc_ref: NodeRef,
        reader: &mut text::BasicReader,
        context: &mut Context,
    );
}

/// An enum of all AST transformers.
#[derive(Debug)]
pub enum AnyAstTransformer {
    TableAstTransformer(TableAstTransformer),
    Extension(Box<dyn AstTransformer>),
}

impl AstTransformer for AnyAstTransformer {
    fn transform(
        &self,
        arena: &mut Arena,
        doc_ref: NodeRef,
        reader: &mut text::BasicReader,
        context: &mut Context,
    ) {
        match self {
            AnyAstTransformer::TableAstTransformer(t) => {
                t.transform(arena, doc_ref, reader, context)
            }
            AnyAstTransformer::Extension(ext) => ext.transform(arena, doc_ref, reader, context),
        }
    }
}

impl From<TableAstTransformer> for AnyAstTransformer {
    fn from(t: TableAstTransformer) -> Self {
        AnyAstTransformer::TableAstTransformer(t)
    }
}

// }}} AstTransformer

// ParagraphTransformer {{{

/// A trait that transforms Paragraph nodes.
pub trait ParagraphTransformer: Debug {
    /// Transforms the given Paragraph node.
    fn transform(
        &self,
        arena: &mut Arena,
        paragraph_ref: NodeRef,
        reader: &mut text::BasicReader,
        context: &mut Context,
    );
}

/// An enum of all Paragraph transformers.
#[derive(Debug)]
#[non_exhaustive]
pub enum AnyParagraphTransformer {
    LinkReferenceParagraphTransformer(LinkReferenceParagraphTransformer),
    TableParagraphTransformer(TableParagraphTransformer),

    TaskListItemParagraphTransformer(TaskListItemParagraphTransformer),

    Extension(Box<dyn ParagraphTransformer>),
}

impl ParagraphTransformer for AnyParagraphTransformer {
    fn transform(
        &self,
        arena: &mut Arena,
        paragraph_ref: NodeRef,
        reader: &mut text::BasicReader,
        context: &mut Context,
    ) {
        match self {
            AnyParagraphTransformer::LinkReferenceParagraphTransformer(t) => {
                t.transform(arena, paragraph_ref, reader, context)
            }
            AnyParagraphTransformer::TableParagraphTransformer(t) => {
                t.transform(arena, paragraph_ref, reader, context)
            }
            AnyParagraphTransformer::TaskListItemParagraphTransformer(t) => {
                t.transform(arena, paragraph_ref, reader, context)
            }
            AnyParagraphTransformer::Extension(ext) => {
                ext.transform(arena, paragraph_ref, reader, context)
            }
        }
    }
}

impl From<LinkReferenceParagraphTransformer> for AnyParagraphTransformer {
    fn from(t: LinkReferenceParagraphTransformer) -> Self {
        AnyParagraphTransformer::LinkReferenceParagraphTransformer(t)
    }
}

impl From<TableParagraphTransformer> for AnyParagraphTransformer {
    fn from(t: TableParagraphTransformer) -> Self {
        AnyParagraphTransformer::TableParagraphTransformer(t)
    }
}

impl From<TaskListItemParagraphTransformer> for AnyParagraphTransformer {
    fn from(t: TaskListItemParagraphTransformer) -> Self {
        AnyParagraphTransformer::TaskListItemParagraphTransformer(t)
    }
}

// }}} ParagraphTransformer

// ParseStack {{{

/// Represents a reference to a element in the parse stack.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseStackElemRef(usize);

/// Represents an element in the parse stack.
#[derive(Debug)]
struct ParseStackElem {
    data: ParseStackElemData,
    next: Option<ParseStackElemRef>,
    previous: Option<ParseStackElemRef>,
    node_ref: NodeRef,
}

impl ParseStackElem {
    /// Creates a new ParseStackElem.
    fn new(data: impl Into<ParseStackElemData>, node_ref: NodeRef) -> Self {
        Self {
            data: data.into(),
            next: None,
            previous: None,
            node_ref,
        }
    }

    /// Returns the data of the parse stack element.
    #[inline(always)]
    #[allow(unused)]
    fn data(&self) -> &ParseStackElemData {
        &self.data
    }

    /// Returns mutable reference to the data of the parse stack element.
    #[inline(always)]
    #[allow(unused)]
    fn data_mut(&mut self) -> &mut ParseStackElemData {
        &mut self.data
    }

    /// Returns the next parse stack element reference.
    #[inline(always)]
    fn next(&self) -> Option<ParseStackElemRef> {
        self.next
    }

    /// Returns the previous parse stack element reference.
    #[inline(always)]
    fn previous(&self) -> Option<ParseStackElemRef> {
        self.previous
    }

    /// Returns the node reference of the parse stack element.
    #[inline(always)]
    fn node_ref(&self) -> NodeRef {
        self.node_ref
    }

    #[inline(always)]
    fn index(&self, arena: &Arena) -> text::Index {
        as_kind_data!(arena, self.node_ref(), Text)
            .index()
            .copied()
            .unwrap()
    }

    fn remaining(&self, arena: &Arena) -> Option<text::Index> {
        match &self.data {
            ParseStackElemData::Delimiter(delim) => {
                if delim.remaining() == 0 {
                    None
                } else {
                    let t = as_kind_data!(arena, self.node_ref(), Text);
                    Some(text::Index::new(
                        t.index().unwrap().start(),
                        t.index().unwrap().start() + delim.remaining(),
                    ))
                }
            }
            ParseStackElemData::LinkLabel(label) => {
                let t = as_kind_data!(arena, self.node_ref(), Text);
                if label.is_consumed() {
                    None
                } else {
                    Some(t.index().copied().unwrap())
                }
            }
            ParseStackElemData::LinkBottom(_) => None,
        }
    }
}

/// An enum of parse stack data for CommonMark builtin elements.
#[derive(Debug)]
enum ParseStackElemData {
    Delimiter(Delimiter),

    LinkLabel(LinkLabel),

    LinkBottom(ParseStackElemRef),
}

/// A trait for packing and unpacking parse stack elements.
trait ParseStackElemSpec {
    /// The item type of the parse stack element.
    type Item;

    /// Packs the item into a parse stack element data.
    fn pack(x: Self::Item) -> ParseStackElemData;

    /// Unpacks the item from a parse stack element data.
    fn unpack(d: &ParseStackElemData) -> Option<&Self::Item>;
}

/// A tag for [`Delimiter`] parse stack elements.
#[derive(Debug)]
struct DelimiterTag;

impl ParseStackElemSpec for DelimiterTag {
    type Item = Delimiter;

    fn pack(x: Self::Item) -> ParseStackElemData {
        ParseStackElemData::Delimiter(x)
    }

    fn unpack(d: &ParseStackElemData) -> Option<&Self::Item> {
        match d {
            ParseStackElemData::Delimiter(delim) => Some(delim),
            _ => None,
        }
    }
}

/// A tag for [`LinkLabel`] parse stack elements.
#[derive(Debug)]
struct LinkLabelTag;

impl ParseStackElemSpec for LinkLabelTag {
    type Item = LinkLabel;

    fn pack(x: Self::Item) -> ParseStackElemData {
        ParseStackElemData::LinkLabel(x)
    }

    fn unpack(_d: &ParseStackElemData) -> Option<&Self::Item> {
        match _d {
            ParseStackElemData::LinkLabel(bracket) => Some(bracket),
            _ => None,
        }
    }
}

/// A tag for link bottom parse stack elements.
#[derive(Debug)]
struct LinkBottomTag;

impl ParseStackElemSpec for LinkBottomTag {
    type Item = ParseStackElemRef;

    fn pack(x: Self::Item) -> ParseStackElemData {
        ParseStackElemData::LinkBottom(x)
    }

    fn unpack(d: &ParseStackElemData) -> Option<&Self::Item> {
        match d {
            ParseStackElemData::LinkBottom(ref_elem) => Some(ref_elem),
            _ => None,
        }
    }
}

/// Callbacks for processing delimiters.
#[derive(Clone, Debug)]
pub struct DelimiterProcessor {
    is_delimiter: fn(u8) -> bool,
    can_open_closer: fn(&Delimiter, &Delimiter) -> bool,
    on_match: fn(&mut Arena, usize) -> NodeRef,
}

impl DelimiterProcessor {
    /// Creates a new DelimiterProcessor.
    pub fn new(
        is_delimiter: fn(u8) -> bool,
        can_open_closer: fn(&Delimiter, &Delimiter) -> bool,
        on_match: fn(&mut Arena, usize) -> NodeRef,
    ) -> Self {
        Self {
            is_delimiter,
            can_open_closer,
            on_match,
        }
    }

    /// Returns true if the given character is a delimiter, otherwise false.
    #[inline(always)]
    pub fn is_delimiter(&self, ch: u8) -> bool {
        (self.is_delimiter)(ch)
    }

    /// Returns true if the given opener can open the given closer, otherwise false.
    #[inline(always)]
    pub fn can_open_closer(&self, opener: &Delimiter, closer: &Delimiter) -> bool {
        (self.can_open_closer)(opener, closer)
    }

    /// Calls when a match is found and returns a new NodeRef.
    #[inline(always)]
    pub fn on_match(&self, arena: &mut Arena, count: usize) -> NodeRef {
        (self.on_match)(arena, count)
    }
}

/// Represents a delimiter.
/// A Delimiter is a parse stack element that has same opener and closer characters.
/// CommonMark emphasis syntax uses delimiters.
#[derive(Debug)]
pub struct Delimiter {
    char: u8,
    length: usize,
    consumed: usize,
    can_open: bool,
    can_close: bool,
    processor: DelimiterProcessor,
}

impl Delimiter {
    /// Creates a new [`Delimiter`].
    pub fn new(
        char: u8,
        length: usize,
        can_open: bool,
        can_close: bool,
        processor: DelimiterProcessor,
    ) -> Self {
        Self {
            char,
            length,
            consumed: 0,
            can_open,
            can_close,
            processor,
        }
    }

    /// Returns the number of remaining delimiters.
    #[inline(always)]
    pub fn remaining(&self) -> usize {
        self.length - self.consumed
    }

    /// Returns the delimiter character.
    #[inline(always)]
    pub fn char(&self) -> u8 {
        self.char
    }

    /// Returns the total length of the delimiter.
    #[inline(always)]
    pub fn length(&self) -> usize {
        self.length
    }

    /// Returns the number of consumed delimiters.
    #[inline(always)]
    pub fn consumed(&self) -> usize {
        self.consumed
    }

    /// Consumes the given number of delimiters.
    #[inline(always)]
    pub fn consume(&mut self, count: usize) {
        self.consumed += count;
    }

    /// Returns true if the delimiter can open emphasis, otherwise false.
    #[inline(always)]
    pub fn can_open(&self) -> bool {
        self.can_open
    }

    /// Returns true if the delimiter can close emphasis, otherwise false.
    #[inline(always)]
    pub fn can_close(&self) -> bool {
        self.can_close
    }

    /// Returns the processor of the delimiter.
    #[inline(always)]
    pub fn processor(&self) -> &DelimiterProcessor {
        &self.processor
    }

    /// Calculates how many characters should be used for opening
    /// a new span correspond to given closer.
    pub fn calc_consumption(&self, closer: &Delimiter) -> usize {
        if (self.can_close() || closer.can_open())
            && (self.length + closer.length).is_multiple_of(3)
            && !closer.length.is_multiple_of(3)
        {
            return 0;
        }
        if self.remaining() >= 2 && closer.remaining() >= 2 {
            return 2;
        }
        1
    }
}

#[derive(Debug)]
struct LinkLabel {
    is_image: bool,
    is_consumed: bool,
}

impl LinkLabel {
    /// Creates a new LinkLabel.
    pub fn new(is_image: bool) -> Self {
        Self {
            is_image,
            is_consumed: false,
        }
    }

    /// Returns true if the link label is for an image, otherwise false.
    #[inline(always)]
    pub fn is_image(&self) -> bool {
        self.is_image
    }

    /// Returns true if the link label is already consumed, otherwise false.
    pub fn is_consumed(&self) -> bool {
        self.is_consumed
    }

    /// Consumes the link label.
    pub fn consume(&mut self) {
        self.is_consumed = true
    }
}

/// A parse stack that holds parse stack elements.
#[derive(Debug)]
struct ParseStack<S: ParseStackElemSpec> {
    bottom: Option<ParseStackElemRef>,
    top: Option<ParseStackElemRef>,
    elements: Vec<Option<ParseStackElem>>,
    _spec: PhantomData<S>,
}

impl<S: ParseStackElemSpec> Default for ParseStack<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ParseStackElemSpec> ParseStack<S> {
    /// Creates a new ParseStack.
    pub fn new() -> Self {
        Self {
            elements: Vec::with_capacity(128),
            bottom: None,
            top: None,
            _spec: PhantomData,
        }
    }

    /// Pushes a new element to the parse stack and returns its reference.
    pub fn push(&mut self, elem: S::Item, node_ref: NodeRef) -> ParseStackElemRef {
        let prev = self.top;
        let idx = match prev {
            Some(r) => r.0 + 1,
            None => 0,
        };
        if idx >= self.elements.len() {
            self.elements.push(None);
        }
        let mut elem = ParseStackElem::new(S::pack(elem), node_ref);
        elem.previous = prev;
        if let Some(prev) = prev {
            if let Some(prev_elem) = self.elements.get_mut(prev.0).and_then(|e| e.as_mut()) {
                prev_elem.next = Some(ParseStackElemRef(idx));
            }
        }
        self.elements[idx] = Some(elem);
        self.top = Some(ParseStackElemRef(idx));
        if idx == 0 {
            self.bottom = self.top
        }
        self.top.unwrap()
    }

    /// Returns reference to the parse stack element.
    pub fn get_elem(&self, r: ParseStackElemRef) -> Option<&ParseStackElem> {
        self.elements.get(r.0).and_then(|e| e.as_ref())
    }

    /// Returns reference to the parse stack element.
    /// Panics if the reference is invalid.
    pub fn elem(&self, r: ParseStackElemRef) -> &ParseStackElem {
        self.get_elem(r).expect("Invalid ParseStackElemRef")
    }

    fn get_elem_mut(&mut self, r: ParseStackElemRef) -> Option<&mut ParseStackElem> {
        self.elements.get_mut(r.0).and_then(|e| e.as_mut())
    }

    /// Returns the data of the parse stack element.
    pub fn get_data(&self, r: ParseStackElemRef) -> Option<&S::Item> {
        self.get_elem(r).and_then(|e| S::unpack(&e.data))
    }

    /// Returns the data of the parse stack element.
    /// Panics if the reference is invalid or the data type is mismatched.
    pub fn data(&self, r: ParseStackElemRef) -> &S::Item {
        self.get_data(r)
            .expect("Invalid ParseStackElemRef or data type mismatch")
    }

    /// Returns mutable reference to the data of the parse stack element.
    pub fn get_data_mut(&mut self, r: ParseStackElemRef) -> Option<&mut S::Item> {
        self.get_elem_mut(r).and_then(|e| match &mut e.data {
            d if S::unpack(d).is_some() => {
                // This is safe because we have checked that d is of type S::Item
                unsafe {
                    let ptr = d as *mut ParseStackElemData;
                    let item_ptr = ptr as *mut S::Item;
                    Some(&mut *item_ptr)
                }
            }
            _ => None,
        })
    }

    /// Returns mutable reference to the data of the parse stack element.
    /// Panics if the reference is invalid or the data type is mismatched.
    pub fn data_mut(&mut self, r: ParseStackElemRef) -> &mut S::Item {
        self.get_data_mut(r)
            .expect("Invalid ParseStackElemRef or data type mismatch")
    }

    /// Returns the top element of the parse stack.
    pub fn top(&self) -> Option<ParseStackElemRef> {
        self.top
    }

    /// Returns the bottom element of the parse stack.
    pub fn bottom(&self) -> Option<ParseStackElemRef> {
        self.bottom
    }

    /// Removes the top element from the parse stack.
    pub fn remove_top(&mut self, arena: &mut Arena) {
        if let Some(top_r) = self.top {
            self.remove(arena, top_r);
        }
    }

    /// Removes the given element from the parse stack.
    /// If the element was not consumed, add remaining segment back to the parent node.
    pub fn remove(&mut self, arena: &mut Arena, r: ParseStackElemRef) {
        if let Some(elem) = self.elements.get_mut(r.0).and_then(|e| e.take()) {
            if elem.node_ref() != NODE_REF_UNDEFINED {
                if let Some(remaining) = elem.remaining(arena) {
                    if let Some(parent_node) = arena[elem.node_ref()].parent() {
                        let previous_node = arena[elem.node_ref()].previous_sibling();
                        if let Some(prev_node_ref) = previous_node {
                            let parent_node = arena[prev_node_ref].parent().unwrap();
                            parent_node.merge_or_insert_after_text(arena, prev_node_ref, remaining)
                        } else if let Some(first_child_ref) = arena[parent_node].first_child() {
                            parent_node.merge_or_insert_before_text(
                                arena,
                                first_child_ref,
                                remaining,
                            )
                        } else {
                            parent_node.merge_or_append_text(arena, remaining);
                        }
                    }
                }
                elem.node_ref().delete(arena);
            }

            if let Some(prev_r) = elem.previous {
                if let Some(prev_elem) = self.elements.get_mut(prev_r.0).and_then(|e| e.as_mut()) {
                    prev_elem.next = elem.next;
                }
            } else {
                self.bottom = elem.next;
            }
            if let Some(next_r) = elem.next {
                if let Some(next_elem) = self.elements.get_mut(next_r.0).and_then(|e| e.as_mut()) {
                    next_elem.previous = elem.previous;
                }
            } else {
                self.top = elem.previous;
            }
        }
    }

    /// Removes elements from the top until the given element is found.
    pub fn remove_until(&mut self, arena: &mut Arena, r: Option<ParseStackElemRef>) {
        while let Some(top_r) = self.top {
            if Some(top_r) == r {
                break;
            }
            self.remove(arena, top_r);
        }
    }
}

// }}} Stack

// GFM {{{

/// Options for GitHub Flavored Markdown (GFM).
#[derive(Debug, Default)]
pub struct GfmOptions {
    /// Options for GFM autolinks.
    pub linkify: LinkifyOptions,
}

impl ParserOptions for GfmOptions {}

/// Returns a [`ParserExtension`] that adds GitHub Flavored Markdown (GFM) table support.
pub fn gfm_table() -> impl ParserExtension {
    ParserExtensionFn::new(|p: &mut Parser| {
        p.add_ast_transformer(TableAstTransformer::new, NoParserOptions, 0);
        p.add_paragraph_transformer(TableParagraphTransformer::new, NoParserOptions, 200);
    })
}

/// Returns a [`ParserExtension`] that adds GitHub Flavored Markdown (GFM) linkify support.
pub fn gfm_linkify(options: impl Into<LinkifyOptions>) -> impl ParserExtension {
    let options = options.into();
    ParserExtensionFn::new(move |p: &mut Parser| {
        p.add_inline_parser(LinkifyParser::with_options, options, 999);
    })
}

/// Returns a [`ParserExtension`] that adds GitHub Flavored Markdown (GFM) strikethrough support.
pub fn gfm_strikethrough() -> impl ParserExtension {
    ParserExtensionFn::new(|p: &mut Parser| {
        p.add_inline_parser(StrikethroughParser::new, NoParserOptions, 500);
    })
}

/// Returns a [`ParserExtension`] that adds GitHub Flavored Markdown (GFM) task list item support.
pub fn gfm_task_list_item() -> impl ParserExtension {
    ParserExtensionFn::new(|p: &mut Parser| {
        p.add_paragraph_transformer(TaskListItemParagraphTransformer::new, NoParserOptions, 500);
    })
}

/// Returns a [`ParserExtension`] that adds GitHub Flavored Markdown (GFM)
pub fn gfm(options: impl Into<GfmOptions>) -> impl ParserExtension {
    let options = options.into();
    gfm_table()
        .and(gfm_linkify(options.linkify))
        .and(gfm_strikethrough())
        .and(gfm_task_list_item())
}

// }}} GFM

// Tests {{{

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
    use crate::println;

    use crate::text::Index;

    struct Hoge {
        s: String,
    }

    impl Hoge {
        fn set(&mut self, s: impl AsRef<str>) {
            self.s = String::from(s.as_ref());
        }
    }

    #[test]
    fn test_context() {
        let mut context = Context::default();
        let doc = ast::Document::new();
        let doc_data = doc.into();

        let id1 = context.ids_mut().generate("id1", &doc_data);
        let id2 = context.ids_mut().generate("id2", &doc_data);
        println!("id1: {:?}", id1);
        println!("id2: {:?}", id2);

        let source = "Hello, World!";
        let idx: Index = (0, 5).into();
        context.ids_mut().put(idx.str(source));

        let mut hoge = Hoge {
            s: String::from("before"),
        };
        let key1 = String::from("key1");
        hoge.set(&key1);
        println!("key1: {:?}", key1);
    }
}

// }}} Tests
