//! Renderer module for rendering AST to various formats.

pub mod html;

extern crate alloc;

use core::any::TypeId;
use core::fmt::Debug;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use alloc::rc::Rc;

use crate::ast::{self, *};
use crate::context::{self, AnyValueSpec, ContextKey, ContextKeyRegistry};
use crate::error::{CallbackError, Error, Result};
use crate::util::{HashMap, Prioritized};

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;

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
}
// }}} Context

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

    tmp_pre_render_hooks: Option<Vec<Prioritized<BoxPreRender<'r, W>>>>,
    pre_render_hooks: Option<Vec<BoxPreRender<'r, W>>>,

    tmp_post_render_hooks: Option<Vec<Prioritized<BoxPostRender<'r, W>>>>,
    post_render_hooks: Option<Vec<BoxPostRender<'r, W>>>,

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

    table_override: bool,
    table_header_override: bool,
    table_body_override: bool,
    table_row_override: bool,
    table_cell_override: bool,

    text_override: bool,
    code_span_override: bool,
    emphasis_override: bool,
    link_override: bool,
    image_override: bool,
    raw_html_override: bool,

    strikethrough_override: bool,
}

/// A trait for registering node renderers.
pub trait NodeRendererRegistry<'r, W> {
    fn register_node_renderer_fn(&mut self, type_id: TypeId, renderer_fn: BoxRenderNode<'r, W>);
}

impl<'r, W, B: BuiltinNodesRenderer<W>, O: FormatOptions> NodeRendererRegistry<'r, W>
    for RendererHelper<'r, W, B, O>
{
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
            tmp_pre_render_hooks: None,
            pre_render_hooks: None,
            tmp_post_render_hooks: None,
            post_render_hooks: None,
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

            table_override: false,
            table_header_override: false,
            table_body_override: false,
            table_row_override: false,
            table_cell_override: false,

            text_override: false,
            code_span_override: false,
            emphasis_override: false,
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
        if let Some(mut tmp_hooks) = self.tmp_pre_render_hooks.take() {
            tmp_hooks.sort();
            self.pre_render_hooks = Some(Vec::with_capacity(tmp_hooks.len()));
            for mut h in tmp_hooks.drain(..) {
                self.pre_render_hooks.as_mut().unwrap().push(h.take());
            }
        }
        if let Some(mut tmp_hooks) = self.tmp_post_render_hooks.take() {
            tmp_hooks.sort();
            self.post_render_hooks = Some(Vec::with_capacity(tmp_hooks.len()));
            for mut h in tmp_hooks.drain(..) {
                self.post_render_hooks.as_mut().unwrap().push(h.take());
            }
        }
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

    /// Adds a pre render hook.
    ///
    /// `F` is a function or closure that constructs a node renderer.
    /// F can take 0 to 4 arguments, each of which can be one of the following types:
    ///
    /// - `()`
    /// - `O`: The format options type.
    /// - `T`: The renderer options type.
    /// - `Rc<RefCell<ContextKeyRegistry>>`
    /// - `Rc<RefCell<NodeKindRegistry>>`
    pub fn add_pre_render_hook<A, T, R, F>(&mut self, f: F, ropt: T, priority: u32)
    where
        T: RendererOptions,
        F: RendererConstructor<A, O, T, R>,
        R: PreRender<W> + 'r,
    {
        let hook = f.call(&RendererConstructorOptions {
            format_options: self.format_options.clone(),
            context_registry: self.context_key_registry.clone(),
            node_kind_registry: self.node_kinds.clone(),
            ropt: Cell::new(Some(ropt)),
        });
        if self.tmp_pre_render_hooks.is_none() {
            self.tmp_pre_render_hooks = Some(Vec::new());
        }
        self.tmp_pre_render_hooks
            .as_mut()
            .unwrap()
            .push(Prioritized::new(BoxPreRender::new(hook), priority));
    }

    /// Adds a post render hook.
    ///
    /// `F` is a function or closure that constructs a node renderer.
    /// F can take 0 to 4 arguments, each of which can be one of the following types:
    ///
    /// - `()`
    /// - `O`: The format options type.
    /// - `T`: The renderer options type.
    /// - `Rc<RefCell<ContextKeyRegistry>>`
    /// - `Rc<RefCell<NodeKindRegistry>>`
    pub fn add_post_render_hook<A, T, R, F>(&mut self, f: F, ropt: T, priority: u32)
    where
        T: RendererOptions,
        F: RendererConstructor<A, O, T, R>,
        R: PostRender<W> + 'r,
    {
        let hook = f.call(&RendererConstructorOptions {
            format_options: self.format_options.clone(),
            context_registry: self.context_key_registry.clone(),
            node_kind_registry: self.node_kinds.clone(),
            ropt: Cell::new(Some(ropt)),
        });
        if self.tmp_post_render_hooks.is_none() {
            self.tmp_post_render_hooks = Some(Vec::new());
        }
        self.tmp_post_render_hooks
            .as_mut()
            .unwrap()
            .push(Prioritized::new(BoxPostRender::new(hook), priority));
    }

    /// Renders the AST to the given writer.
    pub fn render<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
    ) -> Result<()> {
        let context = &mut Context::new();
        let reg = self.context_key_registry.borrow();
        context.initialize(&reg);
        let render = RenrererHelperRender { helper: self };

        if let Some(pre_hooks) = &self.pre_render_hooks {
            for hook in pre_hooks {
                hook.pre_render(writer, source, arena, node_ref, &render, context)?;
            }
        }
        render.render(writer, source, arena, node_ref, context)?;
        if let Some(post_hooks) = &self.post_render_hooks {
            for hook in post_hooks {
                hook.post_render(writer, source, arena, node_ref, &render, context)?;
            }
        }
        Ok(())
    }

    fn render_node<'a>(
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

    /// Renders a emphasis node.
    fn render_emphasis<'a>(
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

// Hook & RenderNode {{{

/// A trait for rendering a node.
pub trait Render<W> {
    fn render(
        &self,
        writer: &mut W,
        source: &str,
        arena: &Arena,
        node_ref: NodeRef,
        context: &mut Context,
    ) -> Result<()>;
}

struct RenrererHelperRender<'r, W, B: BuiltinNodesRenderer<W>, O: FormatOptions> {
    helper: &'r RendererHelper<'r, W, B, O>,
}

impl<'r, W, B: BuiltinNodesRenderer<W>, O: FormatOptions> Render<W>
    for RenrererHelperRender<'r, W, B, O>
{
    fn render(
        &self,
        writer: &mut W,
        source: &str,
        arena: &Arena,
        node_ref: NodeRef,
        context: &mut Context,
    ) -> Result<()> {
        walk(arena, node_ref, &mut |arena: &Arena,
                                    node_ref: NodeRef,
                                    entering: bool|
         -> Result<WalkStatus> {
            self.helper
                .render_node(writer, source, arena, node_ref, entering, context)
        })
        .map(|_| ())
        .map_err(|e| match e {
            CallbackError::Internal(err) => err,
            CallbackError::Callback(err) => err,
        })?;
        Ok(())
    }
}

/// A trait for pre-rendering nodes.
pub trait PreRender<W> {
    fn pre_render(
        &self,
        writer: &mut W,
        source: &str,
        arena: &Arena,
        node_ref: NodeRef,
        render: &dyn Render<W>,
        context: &mut Context,
    ) -> Result<()>;
}

impl<F, W> PreRender<W> for F
where
    F: Fn(&mut W, &str, &Arena, NodeRef, &dyn Render<W>, &mut Context) -> Result<()>,
{
    fn pre_render(
        &self,
        writer: &mut W,
        source: &str,
        arena: &Arena,
        node_ref: NodeRef,
        render: &dyn Render<W>,
        context: &mut Context,
    ) -> Result<()> {
        (self)(writer, source, arena, node_ref, render, context)
    }
}

/// A trait for post-rendering nodes.
pub trait PostRender<W> {
    fn post_render(
        &self,
        writer: &mut W,
        source: &str,
        arena: &Arena,
        node_ref: NodeRef,
        render: &dyn Render<W>,
        context: &mut Context,
    ) -> Result<()>;
}

impl<F, W> PostRender<W> for F
where
    F: Fn(&mut W, &str, &Arena, NodeRef, &dyn Render<W>, &mut Context) -> Result<()>,
{
    fn post_render(
        &self,
        writer: &mut W,
        source: &str,
        arena: &Arena,
        node_ref: NodeRef,
        render: &dyn Render<W>,
        context: &mut Context,
    ) -> Result<()> {
        (self)(writer, source, arena, node_ref, render, context)
    }
}

/// A boxed pre-render function.
pub struct BoxPreRender<'r, W>(Box<dyn PreRender<W> + 'r>);

impl<'r, W> BoxPreRender<'r, W> {
    /// Creates a new `BoxPreRender` from a given function or closure.
    pub fn new(pr: impl PreRender<W> + 'r) -> Self {
        BoxPreRender(Box::new(pr))
    }

    #[inline(always)]
    pub fn pre_render(
        &self,
        writer: &mut W,
        source: &str,
        arena: &Arena,
        node_ref: NodeRef,
        render: &dyn Render<W>,
        context: &mut Context,
    ) -> Result<()> {
        self.0
            .pre_render(writer, source, arena, node_ref, render, context)
    }
}

impl<W> Debug for BoxPreRender<'_, W> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BoxPreRender")
    }
}

/// A boxed post-render function.
pub struct BoxPostRender<'r, W>(Box<dyn PostRender<W> + 'r>);

impl<'r, W> BoxPostRender<'r, W> {
    /// Creates a new `BoxPostRender` from a given function or closure.
    pub fn new(pr: impl PostRender<W> + 'r) -> Self {
        BoxPostRender(Box::new(pr))
    }

    #[inline(always)]
    pub fn post_render(
        &self,
        writer: &mut W,
        source: &str,
        arena: &Arena,
        node_ref: NodeRef,
        render: &dyn Render<W>,
        context: &mut Context,
    ) -> Result<()> {
        self.0
            .post_render(writer, source, arena, node_ref, render, context)
    }
}

impl<W> Debug for BoxPostRender<'_, W> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BoxPostRender")
    }
}

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
