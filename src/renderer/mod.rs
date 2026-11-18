//! Renderer module for rendering AST to various formats.

pub mod html;

extern crate alloc;

use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::{
    any::TypeId,
    cell::{Cell, RefCell},
    fmt::Debug,
};

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;
use crate::{
    ast::{self, *},
    context::{self, AnyValueSpec, ContextKey, ContextKeyRegistry},
    error::{CallbackError, Error, Result},
    util::HashMap,
};

// TextWrite {{{

/// Output trait for writing text.
/// This trait is subset of `core::fmt::Write`.
pub trait TextWrite {
    /// Writes a string to the output.
    fn write_str(&mut self, s: &str) -> Result<()>;

    /// Writes a character to the output.
    #[inline(always)]
    fn write_char(&mut self, c: char) -> Result<()> {
        self.write_str(c.encode_utf8(&mut [0; 4]))
    }
}

impl<W: core::fmt::Write> TextWrite for W {
    #[inline(always)]
    fn write_str(&mut self, s: &str) -> Result<()> {
        self.write_str(s)
            .map_err(|e| Error::io("Failed to write", Some(Box::new(e))))
    }
}

// }}} TextWrite

// Context {{{

/// A context for rendering operations.
pub struct Context {
    base: context::Context,

    task: Option<Task>,

    pblock: Vec<bool>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    /// Creates a new `Context`.
    pub fn new() -> Self {
        Context {
            base: context::Context::new(),
            task: None,
            pblock: Vec::with_capacity(16),
        }
    }

    fn initialize(&mut self, reg: &ContextKeyRegistry) {
        self.base.initialize(reg);
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

    fn pop_task(&mut self) -> Option<Task> {
        let task = self.task;
        self.task = None;
        task
    }

    fn set_task(&mut self, task: Option<Task>) {
        self.task = task;
    }

    fn push_pblock(&mut self, value: bool) {
        self.pblock.push(value);
    }

    fn pop_pblock(&mut self) -> Option<bool> {
        self.pblock.pop()
    }
}
// }}} Context

// RenderOptions {{{

/// Trait for initializing the [`Context`] before rendering.
pub trait InitializeContext {
    /// Initializes the given context.
    fn initialize(&mut self, context: &mut Context, registry: &ContextKeyRegistry);
}

impl<F> InitializeContext for F
where
    F: FnMut(&mut Context, &ContextKeyRegistry),
{
    fn initialize(&mut self, context: &mut Context, registry: &ContextKeyRegistry) {
        self(context, registry)
    }
}

/// A trait for initializing the [`ContextKeyRegistry`] of a renderer.
/// This is called once when the renderer is created, so that you can register
/// context keys before rendering.
pub trait InitializeContextKeyRegistry {
    /// Initializes the given registry.
    fn initialize(&self, registry: &mut ContextKeyRegistry);
}

impl<F> InitializeContextKeyRegistry for F
where
    F: Fn(&mut ContextKeyRegistry),
{
    fn initialize(&self, registry: &mut ContextKeyRegistry) {
        self(registry)
    }
}

/// Options for a single call of `render`.
#[derive(Default)]
pub struct RenderOptions<'a> {
    /// A function that initializes the [`Context`] before rendering.
    /// This allows you to share values with renderers.
    pub context_initializer: Option<&'a mut dyn InitializeContext>,
}

// }}} RenderOptions

// NodeKind {{{

/// A unique identifier for a node kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NodeKindId(usize);

/// A registry for creating and managing node kinds.
#[derive(Default, Debug)]
pub struct NodeKindRegistry {
    kinds: HashMap<TypeId, usize>,
    current: usize,
    frozen: bool,
}

impl NodeKindRegistry {
    /// Registers a new node kind and returns its unique identifier.
    pub fn register<T: 'static>(&mut self) -> NodeKindId {
        if self.frozen {
            panic!("NodeKindRegistry is frozen and cannot register new kinds");
        }
        let type_id = TypeId::of::<T>();
        self.register_by_type_id(type_id)
    }

    /// Registers a new node kind by its `TypeId` and returns its unique identifier.
    pub fn register_by_type_id(&mut self, type_id: TypeId) -> NodeKindId {
        if self.frozen {
            panic!("NodeKindRegistry is frozen and cannot register new kinds");
        }
        if let Some(&kind) = self.kinds.get(&type_id) {
            NodeKindId(kind)
        } else {
            let kind = self.current;
            self.current += 1;
            self.kinds.insert(type_id, kind);
            NodeKindId(kind)
        }
    }

    /// Gets the unique identifier of a node kind by its type.
    pub fn get<T: 'static>(&self) -> Option<NodeKindId> {
        let type_id = TypeId::of::<T>();
        self.get_by_type_id(type_id)
    }

    /// Gets the unique identifier of a node kind by its `TypeId`.
    pub fn get_by_type_id(&self, type_id: TypeId) -> Option<NodeKindId> {
        if let Some(&kind) = self.kinds.get(&type_id) {
            Some(NodeKindId(kind))
        } else {
            None
        }
    }

    /// Returns the number of registered node kinds.
    pub fn size(&self) -> usize {
        self.current
    }

    /// Freezes the registry, preventing further registrations.
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
}

// }}} NodeKind

// RendererOptions & RendererConstructor {{{

/// A trait for renderer options.
/// Each renderer can define its own options by implementing this trait.
pub trait RendererOptions {}

/// A trait for format options.
pub trait FormatOptions: Clone + Default {}

/// A default implementation of RendererOptions that does nothing.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoRendererOptions;

impl RendererOptions for NoRendererOptions {}

/// Options for constructing a renderer.
pub struct RendererConstructorOptions<O: FormatOptions, T: RendererOptions> {
    /// Options for the output format.
    pub format_options: O,

    ropt: Cell<Option<T>>,
    context_registry: Rc<RefCell<ContextKeyRegistry>>,
    node_kind_registry: Rc<RefCell<NodeKindRegistry>>,
}

trait FromRendererConstructorOptions<O: FormatOptions, T: RendererOptions> {
    fn from_renderer_constructor_options(opt: &RendererConstructorOptions<O, T>) -> Self;
}

impl<O: FormatOptions, T: RendererOptions> FromRendererConstructorOptions<O, T> for T {
    fn from_renderer_constructor_options(opt: &RendererConstructorOptions<O, T>) -> Self {
        opt.ropt.replace(None).unwrap()
    }
}

impl<O: FormatOptions, T: RendererOptions> FromRendererConstructorOptions<O, T>
    for Rc<RefCell<ContextKeyRegistry>>
{
    fn from_renderer_constructor_options(opt: &RendererConstructorOptions<O, T>) -> Self {
        opt.context_registry.clone()
    }
}

impl<O: FormatOptions, T: RendererOptions> FromRendererConstructorOptions<O, T>
    for Rc<RefCell<NodeKindRegistry>>
{
    fn from_renderer_constructor_options(opt: &RendererConstructorOptions<O, T>) -> Self {
        opt.node_kind_registry.clone()
    }
}

/// A trait for constructing renderers with varying arguments.
pub trait RendererConstructor<A, O, T, R>
where
    O: FormatOptions,
    T: RendererOptions,
{
    fn call(self, options: &RendererConstructorOptions<O, T>) -> R;
}

impl<F, O, T, R> RendererConstructor<(), O, T, R> for F
// NoArgs
where
    O: FormatOptions,
    T: RendererOptions,
    F: FnOnce() -> R,
{
    fn call(self, _: &RendererConstructorOptions<O, T>) -> R {
        self()
    }
}

impl<F, A, O, T, R> RendererConstructor<(A,), O, T, R> for F
// 1 Arg
where
    O: FormatOptions,
    T: RendererOptions,
    F: FnOnce(A) -> R,
    A: FromRendererConstructorOptions<O, T>,
{
    fn call(self, options: &RendererConstructorOptions<O, T>) -> R {
        let a = A::from_renderer_constructor_options(options);
        self(a)
    }
}

impl<F, A, B, O, T, R> RendererConstructor<(A, B), O, T, R> for F
// 2 Args
where
    O: FormatOptions,
    T: RendererOptions,
    F: FnOnce(A, B) -> R,
    A: FromRendererConstructorOptions<O, T>,
    B: FromRendererConstructorOptions<O, T>,
{
    fn call(self, options: &RendererConstructorOptions<O, T>) -> R {
        let a = A::from_renderer_constructor_options(options);
        let b = B::from_renderer_constructor_options(options);
        self(a, b)
    }
}

impl<F, A, B, C, O, T, R> RendererConstructor<(A, B, C), O, T, R> for F
// 3 Args
where
    O: FormatOptions,
    T: RendererOptions,
    F: FnOnce(A, B, C) -> R,
    A: FromRendererConstructorOptions<O, T>,
    B: FromRendererConstructorOptions<O, T>,
    C: FromRendererConstructorOptions<O, T>,
{
    fn call(self, options: &RendererConstructorOptions<O, T>) -> R {
        let a = A::from_renderer_constructor_options(options);
        let b = B::from_renderer_constructor_options(options);
        let c = C::from_renderer_constructor_options(options);
        self(a, b, c)
    }
}

impl<F, A, B, C, D, O, T, R> RendererConstructor<(A, B, C, D), O, T, R> for F
// 4 Args
where
    O: FormatOptions,
    T: RendererOptions,
    F: FnOnce(A, B, C, D) -> R,
    A: FromRendererConstructorOptions<O, T>,
    B: FromRendererConstructorOptions<O, T>,
    C: FromRendererConstructorOptions<O, T>,
    D: FromRendererConstructorOptions<O, T>,
{
    fn call(self, options: &RendererConstructorOptions<O, T>) -> R {
        let a = A::from_renderer_constructor_options(options);
        let b = B::from_renderer_constructor_options(options);
        let c = C::from_renderer_constructor_options(options);
        let d = D::from_renderer_constructor_options(options);
        self(a, b, c, d)
    }
}

// }}} RendererOptions & RendererConstructor

// RendererHelper {{{

/// A helper struct for rendering AST.
#[derive(Debug)]
pub struct RendererHelper<'r, W, B: BuiltinNodesRenderer<W>, O: FormatOptions> {
    format_options: O,
    node_kinds: Rc<RefCell<NodeKindRegistry>>,
    context_key_registry: Rc<RefCell<ContextKeyRegistry>>,
    node_renderers: Vec<Option<BoxRenderNode<'r, W>>>,
    // decorators per node kind, in registration order(the last one is the outermost).
    node_decorators: Vec<Vec<BoxRenderNodeDecorator<'r, W>>>,
    has_node_decorators: bool,

    builtin_node_renderer: B,

    document_override: bool,
    paragraph_override: bool,
    heading_override: bool,
    thematic_break_override: bool,
    code_block_override: bool,
    blockquote_override: bool,
    list_override: bool,
    list_item_override: bool,
    html_block_override: bool,
    link_reference_definition_override: bool,

    table_override: bool,
    table_header_override: bool,
    table_body_override: bool,
    table_row_override: bool,
    table_cell_override: bool,

    text_override: bool,
    code_span_override: bool,
    emphasis_override: bool,
    strong_override: bool,
    link_override: bool,
    image_override: bool,
    raw_html_override: bool,

    strikethrough_override: bool,
}

/// A trait for registering node renderers.
pub trait NodeRendererRegistry<'r, W> {
    fn register_node_renderer_fn(&mut self, type_id: TypeId, renderer_fn: BoxRenderNode<'r, W>);

    /// Registers a decorator for the node kind identified by `type_id`.
    ///
    /// If decorators are already registered for the node kind, the new decorator wraps
    /// the existing ones: it is called first and the existing ones are reachable
    /// through [`NextRenderNode`].
    fn register_node_renderer_decorator_fn(
        &mut self,
        type_id: TypeId,
        decorator: BoxRenderNodeDecorator<'r, W>,
    );
}

impl<'r, W, B: BuiltinNodesRenderer<W>, O: FormatOptions> NodeRendererRegistry<'r, W>
    for RendererHelper<'r, W, B, O>
{
    fn register_node_renderer_decorator_fn(
        &mut self,
        type_id: TypeId,
        decorator: BoxRenderNodeDecorator<'r, W>,
    ) {
        let kind_id = self.node_kinds.borrow_mut().register_by_type_id(type_id);
        if kind_id.0 >= self.node_decorators.len() {
            self.node_decorators.resize_with(kind_id.0 + 1, Vec::new)
        }
        self.node_decorators[kind_id.0].push(decorator);
        self.has_node_decorators = true;
    }

    fn register_node_renderer_fn(&mut self, type_id: TypeId, renderer_fn: BoxRenderNode<'r, W>) {
        let kind_id = self.node_kinds.borrow_mut().register_by_type_id(type_id);
        if kind_id.0 >= self.node_renderers.len() {
            self.node_renderers.resize_with(kind_id.0 + 1, || None)
        }
        self.node_renderers[kind_id.0] = Some(renderer_fn);
        match type_id {
            id if id == TypeId::of::<Document>() => self.document_override = true,
            id if id == TypeId::of::<Paragraph>() => self.paragraph_override = true,
            id if id == TypeId::of::<Heading>() => self.heading_override = true,
            id if id == TypeId::of::<ThematicBreak>() => self.thematic_break_override = true,
            id if id == TypeId::of::<CodeBlock>() => self.code_block_override = true,
            id if id == TypeId::of::<Blockquote>() => self.blockquote_override = true,
            id if id == TypeId::of::<List>() => self.list_override = true,
            id if id == TypeId::of::<ListItem>() => self.list_item_override = true,
            id if id == TypeId::of::<HtmlBlock>() => self.html_block_override = true,

            id if id == TypeId::of::<Table>() => self.table_override = true,
            id if id == TypeId::of::<TableHeader>() => self.table_header_override = true,
            id if id == TypeId::of::<TableBody>() => self.table_body_override = true,
            id if id == TypeId::of::<TableRow>() => self.table_row_override = true,
            id if id == TypeId::of::<TableCell>() => self.table_cell_override = true,

            id if id == TypeId::of::<Text>() => self.text_override = true,
            id if id == TypeId::of::<CodeSpan>() => self.code_span_override = true,
            id if id == TypeId::of::<Emphasis>() => self.emphasis_override = true,
            id if id == TypeId::of::<Strong>() => self.strong_override = true,
            id if id == TypeId::of::<Link>() => self.link_override = true,
            id if id == TypeId::of::<Image>() => self.image_override = true,
            id if id == TypeId::of::<RawHtml>() => self.raw_html_override = true,

            id if id == TypeId::of::<Strikethrough>() => self.strikethrough_override = true,

            _ => {}
        }
    }
}

macro_rules! render_buitin {
    ($self:ident, $type:ty, $override_flag:ident, $builtin_fn:ident, $($args:expr),*) => {
            if $self.$override_flag {
                let kind_id = $self.node_kinds.borrow().get::<$type>().unwrap();
                if let Some(renderer) = &$self.node_renderers[kind_id.0] {
                    return renderer.render_node($($args),*);
                }
            }
            return $self.builtin_node_renderer.$builtin_fn($($args),*);
    };
}

/// A trait for node renderers.
pub trait NodeRenderer<'r, W> {
    fn register_node_renderer_fn(self, nrr: &mut impl NodeRendererRegistry<'r, W>);
}

impl<'r, W, B: BuiltinNodesRenderer<W>, O: FormatOptions> RendererHelper<'r, W, B, O> {
    /// Creates a new [`RendererHelper`] .
    pub fn new(format_options: O, builtin_node_renderer: B) -> Self {
        RendererHelper {
            format_options,
            node_kinds: Rc::new(RefCell::new(NodeKindRegistry::default())),
            context_key_registry: Rc::new(RefCell::new(ContextKeyRegistry::default())),
            node_renderers: Vec::new(),
            node_decorators: Vec::new(),
            has_node_decorators: false,
            builtin_node_renderer,
            document_override: false,
            paragraph_override: false,
            heading_override: false,
            thematic_break_override: false,
            code_block_override: false,
            blockquote_override: false,
            list_override: false,
            list_item_override: false,
            html_block_override: false,
            link_reference_definition_override: false,

            table_override: false,
            table_header_override: false,
            table_body_override: false,
            table_row_override: false,
            table_cell_override: false,

            text_override: false,
            code_span_override: false,
            emphasis_override: false,
            strong_override: false,
            link_override: false,
            image_override: false,
            raw_html_override: false,

            strikethrough_override: false,
        }
    }

    /// Initializes the renderer helper.
    ///
    /// Renderer implementations should call this method in their constructors.
    pub fn init(&mut self) {
        self.node_kinds.borrow_mut().freeze();
    }

    /// Adds a node renderer.
    ///
    /// `F` is a function or closure that constructs a node renderer.
    /// F can take 0 to 4 arguments, each of which can be one of the following types:
    ///
    /// - `()`
    /// - `O`: The format options type.
    /// - `T`: The renderer options type.
    /// - `Rc<RefCell<ContextKeyRegistry>>`
    /// - `Rc<RefCell<NodeKindRegistry>>`
    pub fn add_node_renderer<A, T, R, F>(&mut self, f: F, ropt: T)
    where
        T: RendererOptions,
        F: RendererConstructor<A, O, T, R>,
        R: NodeRenderer<'r, W>,
    {
        let renderer = f.call(&RendererConstructorOptions {
            format_options: self.format_options.clone(),
            context_registry: self.context_key_registry.clone(),
            node_kind_registry: self.node_kinds.clone(),
            ropt: Cell::new(Some(ropt)),
        });
        renderer.register_node_renderer_fn(self)
    }

    /// Renders the AST to the given writer.
    pub fn render<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        options: RenderOptions<'_>,
    ) -> Result<()> {
        let context = &mut Context::new();
        let reg = self.context_key_registry.borrow();
        context.initialize(&reg);
        if let Some(initializer) = options.context_initializer {
            initializer.initialize(context, &reg);
        }
        walk(arena, node_ref, &mut |arena: &Arena,
                                    node_ref: NodeRef,
                                    entering: bool|
         -> Result<WalkStatus> {
            self.render_node(writer, source, arena, node_ref, entering, context)
        })
        .map(|_| ())
        .map_err(|e| match e {
            CallbackError::Internal(err) => err,
            CallbackError::Callback(err) => err,
        })
    }

    #[inline(always)]
    fn render_node<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        if self.has_node_decorators {
            if let Some(kind_id) = self.decorated_kind_id(arena[node_ref].kind_data()) {
                let n = self.node_decorators[kind_id].len();
                return self.render_decorated(
                    kind_id, n, writer, source, arena, node_ref, entering, context,
                );
            }
        }
        self.render_node_undecorated(writer, source, arena, node_ref, entering, context)
    }

    // Returns the kind id if decorators are registered for the kind of the given data.
    #[cold]
    fn decorated_kind_id(&self, kd: &KindData) -> Option<usize> {
        let type_id = match kd {
            KindData::Document(_) => TypeId::of::<Document>(),
            KindData::Paragraph(_) => TypeId::of::<Paragraph>(),
            KindData::Heading(_) => TypeId::of::<Heading>(),
            KindData::ThematicBreak(_) => TypeId::of::<ThematicBreak>(),
            KindData::CodeBlock(_) => TypeId::of::<CodeBlock>(),
            KindData::Blockquote(_) => TypeId::of::<Blockquote>(),
            KindData::List(_) => TypeId::of::<List>(),
            KindData::ListItem(_) => TypeId::of::<ListItem>(),
            KindData::HtmlBlock(_) => TypeId::of::<HtmlBlock>(),
            KindData::LinkReferenceDefinition(_) => TypeId::of::<LinkReferenceDefinition>(),
            KindData::Table(_) => TypeId::of::<Table>(),
            KindData::TableHeader(_) => TypeId::of::<TableHeader>(),
            KindData::TableBody(_) => TypeId::of::<TableBody>(),
            KindData::TableRow(_) => TypeId::of::<TableRow>(),
            KindData::TableCell(_) => TypeId::of::<TableCell>(),
            KindData::Text(_) => TypeId::of::<Text>(),
            KindData::CodeSpan(_) => TypeId::of::<CodeSpan>(),
            KindData::Emphasis(_) => TypeId::of::<Emphasis>(),
            KindData::Strong(_) => TypeId::of::<Strong>(),
            KindData::Link(_) => TypeId::of::<Link>(),
            KindData::Image(_) => TypeId::of::<Image>(),
            KindData::RawHtml(_) => TypeId::of::<RawHtml>(),
            KindData::Strikethrough(_) => TypeId::of::<Strikethrough>(),
            KindData::Extension(ext) => (**ext).type_id(),
        };
        let id = self.node_kinds.borrow().get_by_type_id(type_id)?.0;
        match self.node_decorators.get(id) {
            Some(d) if !d.is_empty() => Some(id),
            _ => None,
        }
    }

    // Renders a node through the first `n` decorators of the given kind.
    // The `n`-th decorator is the outermost one.
    #[allow(clippy::too_many_arguments)]
    fn render_decorated<'a>(
        &self,
        kind_id: usize,
        n: usize,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        if n == 0 {
            return self
                .render_node_undecorated(writer, source, arena, node_ref, entering, context);
        }
        let next = NextRenderNode {
            chain: self,
            kind_id,
            n: n - 1,
        };
        self.node_decorators[kind_id][n - 1]
            .0
            .decorate_node(&next, writer, source, arena, node_ref, entering, context)
    }

    fn render_node_undecorated<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        match arena[node_ref].kind_data() {
            KindData::Document(_) => {
                render_buitin!(
                    self,
                    Document,
                    document_override,
                    render_document,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Paragraph(_) => {
                render_buitin!(
                    self,
                    Paragraph,
                    paragraph_override,
                    render_paragraph,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Heading(_) => {
                render_buitin!(
                    self,
                    Heading,
                    heading_override,
                    render_heading,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::ThematicBreak(_) => {
                render_buitin!(
                    self,
                    ThematicBreak,
                    thematic_break_override,
                    render_thematic_break,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::CodeBlock(_) => {
                render_buitin!(
                    self,
                    CodeBlock,
                    code_block_override,
                    render_code_block,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Blockquote(_) => {
                render_buitin!(
                    self,
                    Blockquote,
                    blockquote_override,
                    render_blockquote,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::List(_) => {
                render_buitin!(
                    self,
                    List,
                    list_override,
                    render_list,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::ListItem(_) => {
                render_buitin!(
                    self,
                    ListItem,
                    list_item_override,
                    render_list_item,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::HtmlBlock(_) => {
                render_buitin!(
                    self,
                    HtmlBlock,
                    html_block_override,
                    render_html_block,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::LinkReferenceDefinition(_) => {
                render_buitin!(
                    self,
                    LinkReferenceDefinition,
                    link_reference_definition_override,
                    render_link_reference_definition,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Table(_) => {
                render_buitin!(
                    self,
                    Table,
                    table_override,
                    render_table,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::TableHeader(_) => {
                render_buitin!(
                    self,
                    TableHeader,
                    table_header_override,
                    render_table_header,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::TableBody(_) => {
                render_buitin!(
                    self,
                    TableBody,
                    table_body_override,
                    render_table_body,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::TableRow(_) => {
                render_buitin!(
                    self,
                    TableRow,
                    table_row_override,
                    render_table_row,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::TableCell(_) => {
                render_buitin!(
                    self,
                    TableCell,
                    table_cell_override,
                    render_table_cell,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Text(_) => {
                render_buitin!(
                    self,
                    Text,
                    text_override,
                    render_text,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::CodeSpan(_) => {
                render_buitin!(
                    self,
                    CodeSpan,
                    code_span_override,
                    render_code_span,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Emphasis(_) => {
                render_buitin!(
                    self,
                    Emphasis,
                    emphasis_override,
                    render_emphasis,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Strong(_) => {
                render_buitin!(
                    self,
                    Strong,
                    strong_override,
                    render_strong,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Link(_) => {
                render_buitin!(
                    self,
                    Link,
                    link_override,
                    render_link,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Image(_) => {
                render_buitin!(
                    self,
                    Image,
                    image_override,
                    render_image,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::RawHtml(_) => {
                render_buitin!(
                    self,
                    RawHtml,
                    raw_html_override,
                    render_raw_html,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Strikethrough(_) => {
                render_buitin!(
                    self,
                    Strikethrough,
                    strikethrough_override,
                    render_strikethrough,
                    writer,
                    source,
                    arena,
                    node_ref,
                    entering,
                    context
                );
            }
            KindData::Extension(ext) => {
                let type_id = (**ext).type_id();
                let kind_id = self
                    .node_kinds
                    .borrow()
                    .get_by_type_id(type_id)
                    .unwrap_or(NodeKindId(usize::MAX));
                if kind_id.0 < self.node_renderers.len() {
                    if let Some(renderer) = &self.node_renderers[kind_id.0] {
                        return renderer
                            .render_node(writer, source, arena, node_ref, entering, context);
                    }
                }
                Ok(WalkStatus::Continue)
            }
        }
    }
}

impl<'r, W, B: BuiltinNodesRenderer<W>, O: FormatOptions> RenderChain<W>
    for RendererHelper<'r, W, B, O>
{
    fn render_decorated<'a>(
        &self,
        kind_id: usize,
        n: usize,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        RendererHelper::render_decorated(
            self, kind_id, n, writer, source, arena, node_ref, entering, context,
        )
    }
}

// }}} RendererHelper

// BuiltinNodesRenderer {{{

/// A trait for rendering built-in nodes.
pub trait BuiltinNodesRenderer<W> {
    /// Renders a document node.
    fn render_document<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a paragraph node.
    fn render_paragraph<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a heading node.
    fn render_heading<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a thematic break node.
    fn render_thematic_break<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a code block node.
    fn render_code_block<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a block quote node.
    fn render_blockquote<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a list node.
    fn render_list<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a list item node.
    fn render_list_item<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders an html block node.
    fn render_html_block<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a link reference definition node.
    fn render_link_reference_definition<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a table node.
    fn render_table<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a table header node.
    fn render_table_header<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a table body node.
    fn render_table_body<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a table row node.
    fn render_table_row<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a table cell node.
    fn render_table_cell<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a text node.
    fn render_text<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a code span node.
    fn render_code_span<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders an emphasis node.
    fn render_emphasis<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a strong node.
    fn render_strong<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a link node.
    fn render_link<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders an image node.
    fn render_image<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a raw html node.
    fn render_raw_html<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;

    /// Renders a strikethrough node.
    fn render_strikethrough<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;
}

// }}} BuiltinNodesRenderer

// RenderNode {{{

/// Traits for rendering nodes.
pub trait RenderNode<W> {
    fn render_node<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;
}

impl<F, W> RenderNode<W> for F
where
    F: for<'a> Fn(&mut W, &'a str, &'a Arena, NodeRef, bool, &mut Context) -> Result<WalkStatus>,
{
    fn render_node<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        (self)(writer, source, arena, node_ref, entering, context)
    }
}

/// A boxed render node function.
pub struct BoxRenderNode<'r, W>(Box<dyn RenderNode<W> + 'r>);

impl<'r, W> BoxRenderNode<'r, W> {
    /// Creates a new `BoxRenderNode` from a given function or closure.
    pub fn new(nrf: impl RenderNode<W> + 'r) -> Self {
        BoxRenderNode(Box::new(nrf))
    }

    #[inline(always)]
    pub fn render_node<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        self.0
            .render_node(writer, source, arena, node_ref, entering, context)
    }
}

impl<W> Debug for BoxRenderNode<'_, W> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BoxRenderNode")
    }
}

// }}} RenderNode

// RenderNodeDecorator {{{

trait RenderChain<W> {
    #[allow(clippy::too_many_arguments)]
    fn render_decorated<'a>(
        &self,
        kind_id: usize,
        n: usize,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;
}

/// The next renderer in a decoration chain.
///
/// Calling [`NextRenderNode::render_node`] calls the next inner decorator, or the node
/// renderer (an overriding one or the builtin one) if there are no more decorators.
pub struct NextRenderNode<'n, W> {
    chain: &'n dyn RenderChain<W>,
    kind_id: usize,
    n: usize,
}

impl<W> NextRenderNode<'_, W> {
    /// Renders the node with the next renderer.
    #[inline]
    pub fn render_node<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        self.chain.render_decorated(
            self.kind_id,
            self.n,
            writer,
            source,
            arena,
            node_ref,
            entering,
            context,
        )
    }
}

/// A decorator of a node renderer.
///
/// A decorator receives the next renderer and decides whether and how to call it,
/// so it can add behaviors before and after the rendering, or replace the rendering.
/// Closures are also decorators.
pub trait RenderNodeDecorator<W> {
    #[allow(clippy::too_many_arguments)]
    fn decorate_node<'a>(
        &self,
        next: &NextRenderNode<'_, W>,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus>;
}

impl<F, W> RenderNodeDecorator<W> for F
where
    F: for<'a> Fn(
        &NextRenderNode<'_, W>,
        &mut W,
        &'a str,
        &'a Arena,
        NodeRef,
        bool,
        &mut Context,
    ) -> Result<WalkStatus>,
{
    fn decorate_node<'a>(
        &self,
        next: &NextRenderNode<'_, W>,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        (self)(next, writer, source, arena, node_ref, entering, context)
    }
}

/// A boxed [`RenderNodeDecorator`].
pub struct BoxRenderNodeDecorator<'r, W>(Box<dyn RenderNodeDecorator<W> + 'r>);

impl<'r, W> BoxRenderNodeDecorator<'r, W> {
    /// Creates a new `BoxRenderNodeDecorator` from a given decorator, function or closure.
    pub fn new(decorator: impl RenderNodeDecorator<W> + 'r) -> Self {
        BoxRenderNodeDecorator(Box::new(decorator))
    }
}

impl<W> Debug for BoxRenderNodeDecorator<'_, W> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BoxRenderNodeDecorator")
    }
}

// }}} RenderNodeDecorator
