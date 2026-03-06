extern crate alloc;

use core::any::TypeId;
use core::fmt::{self, Write};

use rushdown::renderer::html::{Options, Renderer, RendererExtension, RendererExtensionFn};
use rushdown::Result;
use rushdown::{as_extension_data, as_type_data, ast, text::*};
use rushdown::{ast::*, text};
use rushdown::{matches_kind, parser};
use rushdown::{new_markdown_to_html, renderer::*};
use rushdown::{parser::*, renderer};

// UserMention(inline extension) {{{

#[derive(Debug)]
struct UserMention {
    username: text::Value,
}

impl UserMention {
    fn new(username: impl Into<text::Value>) -> Self {
        Self {
            username: username.into(),
        }
    }
}

impl NodeKind for UserMention {
    fn typ(&self) -> NodeType {
        NodeType::Inline
    }

    fn kind_name(&self) -> &'static str {
        "UserMention"
    }
}

impl PrettyPrint for UserMention {
    fn pretty_print(&self, w: &mut dyn Write, source: &str, level: usize) -> fmt::Result {
        writeln!(
            w,
            "{}Username: {}",
            pp_indent(level),
            self.username.str(source)
        )
    }
}

impl From<UserMention> for KindData {
    fn from(e: UserMention) -> Self {
        KindData::Extension(Box::new(e))
    }
}

#[derive(Debug, Default)]
struct UserMentionParser {}

impl UserMentionParser {
    fn new() -> Self {
        Self {}
    }
}

impl InlineParser for UserMentionParser {
    fn trigger(&self) -> &[u8] {
        b"@"
    }

    fn parse(
        &self,
        arena: &mut Arena,
        _parent_ref: NodeRef,
        reader: &mut text::BlockReader,
        _ctx: &mut parser::Context,
    ) -> Option<NodeRef> {
        let (line, seg) = reader.peek_line_bytes()?;
        if line.len() < 2 || !line[1].is_ascii_alphanumeric() {
            return None;
        }
        reader.advance(1); // consume '@'
        let mut username_end = 1;
        while username_end < line.len() && line[username_end].is_ascii_alphanumeric() {
            username_end += 1;
        }
        let username: text::Value = seg
            .with_start(seg.start() + 1)
            .with_stop(seg.start() + username_end)
            .into();
        reader.advance(username_end - 1);
        let node_ref = arena.new_node(UserMention::new(username));
        Some(node_ref)
    }
}

impl From<UserMentionParser> for AnyInlineParser {
    fn from(p: UserMentionParser) -> Self {
        AnyInlineParser::Extension(Box::new(p))
    }
}

#[derive(Debug, Clone)]
struct UserMentionOptions {
    class_name: String,
}

impl Default for UserMentionOptions {
    fn default() -> Self {
        Self {
            class_name: "mention".to_string(),
        }
    }
}

impl RendererOptions for UserMentionOptions {}

struct UserMentionHtmlRenderer<W: TextWrite> {
    _phantom: core::marker::PhantomData<W>,
    writer: html::Writer,
    options: UserMentionOptions,
}

impl<W: TextWrite> UserMentionHtmlRenderer<W> {
    fn with_options(html_opts: Options, options: UserMentionOptions) -> Self {
        Self {
            _phantom: core::marker::PhantomData,
            writer: html::Writer::with_options(html_opts),
            options,
        }
    }
}

impl<W: TextWrite> RenderNode<W> for UserMentionHtmlRenderer<W> {
    fn render_node<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        _context: &mut renderer::Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<span class=\"")?;
            self.writer
                .write_html(w, self.options.class_name.as_str())?;
            self.writer.write_safe_str(w, "\"><strong>")?;
            let um = as_extension_data!(arena, node_ref, UserMention);
            self.writer.write(w, um.username.str(source))?;
        } else {
            self.writer.write_safe_str(w, "</strong></span>")?;
        }
        Ok(WalkStatus::Continue)
    }
}

impl<'cb, W> NodeRenderer<'cb, W> for UserMentionHtmlRenderer<W>
where
    W: TextWrite + 'cb,
{
    fn register_node_renderer_fn(self, nrr: &mut impl NodeRendererRegistry<'cb, W>) {
        nrr.register_node_renderer_fn(TypeId::of::<UserMention>(), BoxRenderNode::new(self));
    }
}

fn user_mention_parser_extension() -> impl ParserExtension {
    ParserExtensionFn::new(|p: &mut Parser| {
        p.add_inline_parser(
            UserMentionParser::new,
            NoParserOptions,
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

// UserMention(inline extension) }}}

// Admonition(AST Transformer extension) {{{

#[derive(Debug)]
struct Admonition {
    adomonition_kind: String,
}

impl Admonition {
    fn new(kind: String) -> Self {
        Self {
            adomonition_kind: kind,
        }
    }

    fn admonition_kind(&self) -> &str {
        &self.adomonition_kind
    }
}

impl NodeKind for Admonition {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "Admonition"
    }
}

impl PrettyPrint for Admonition {
    fn pretty_print(&self, w: &mut dyn Write, _source: &str, level: usize) -> fmt::Result {
        writeln!(
            w,
            "{}AdomonitionKind: {}",
            pp_indent(level),
            self.admonition_kind(),
        )
    }
}

impl From<Admonition> for KindData {
    fn from(e: Admonition) -> Self {
        KindData::Extension(Box::new(e))
    }
}

#[derive(Debug)]
struct AdmonitionParserOptions {
    /// The kinds of admonitions to recognize.
    pub kinds: Vec<String>,
}

impl Default for AdmonitionParserOptions {
    fn default() -> Self {
        Self {
            kinds: vec![
                "note".to_string(),
                "tip".to_string(),
                "warning".to_string(),
                "danger".to_string(),
            ],
        }
    }
}

impl ParserOptions for AdmonitionParserOptions {}

#[derive(Debug)]
struct AdmonitionHtmlRendererOptions {
    /// The CSS class name to use for admonitions.
    pub class_name: String,
}

impl RendererOptions for AdmonitionHtmlRendererOptions {}

impl Default for AdmonitionHtmlRendererOptions {
    fn default() -> Self {
        Self {
            class_name: "markdown-alert".to_string(),
        }
    }
}

#[derive(Debug)]
struct AdmonitionAstTransformer {
    options: AdmonitionParserOptions,
}

impl AdmonitionAstTransformer {
    fn with_options(options: AdmonitionParserOptions) -> Self {
        Self { options }
    }
}

impl AstTransformer for AdmonitionAstTransformer {
    fn transform(
        &self,
        arena: &mut Arena,
        doc_ref: NodeRef,
        reader: &mut text::BasicReader,
        _context: &mut parser::Context,
    ) {
        let mut blockquotes: Vec<NodeRef> = Vec::new();
        ast::walk(arena, doc_ref, &mut |arena: &Arena,
                                        node_ref: NodeRef,
                                        entering: bool|
         -> Result<ast::WalkStatus> {
            if entering {
                if matches_kind!(arena, node_ref, Blockquote) {
                    blockquotes.push(node_ref);
                }
                return Ok(ast::WalkStatus::Continue);
            }
            Ok(ast::WalkStatus::SkipChildren)
        })
        .unwrap();
        for bq_ref in blockquotes {
            let Some(fc) = arena[bq_ref].first_child() else {
                continue;
            };
            if matches_kind!(arena, fc, Paragraph) {
                let bd = as_type_data!(arena, fc, Block);
                if let Some(fl) = bd.lines().iter().next() {
                    let line_text = fl.str(reader.source());
                    for kind in &self.options.kinds {
                        let kind = kind.to_uppercase();
                        let prefix = format!("[!{}]", kind);
                        if line_text.starts_with(&prefix) {
                            let admonition_node_ref = arena.new_node(Admonition::new(kind.clone()));
                            let children: Vec<NodeRef> =
                                arena[bq_ref].children(arena).skip(1).collect();
                            let ad_title_ref = arena.new_node(Paragraph::new());
                            let text_node_ref = arena.new_node(Text::new(kind.to_uppercase()));
                            ad_title_ref.append_child(arena, text_node_ref);
                            admonition_node_ref.append_child(arena, ad_title_ref);
                            for child_ref in children {
                                admonition_node_ref.append_child(arena, child_ref);
                            }
                            arena[bq_ref].parent().unwrap().replace_child(
                                arena,
                                bq_ref,
                                admonition_node_ref,
                            );
                            break;
                        }
                    }
                }
            }
        }
    }
}

impl From<AdmonitionAstTransformer> for AnyAstTransformer {
    fn from(t: AdmonitionAstTransformer) -> Self {
        AnyAstTransformer::Extension(Box::new(t))
    }
}

struct AdmonitionHtmlRenderer<W: TextWrite> {
    _phantom: core::marker::PhantomData<W>,
    writer: html::Writer,
    options: AdmonitionHtmlRendererOptions,
}

impl<W: TextWrite> AdmonitionHtmlRenderer<W> {
    fn with_options(html_opts: Options, options: AdmonitionHtmlRendererOptions) -> Self {
        Self {
            _phantom: core::marker::PhantomData,
            writer: html::Writer::with_options(html_opts),
            options,
        }
    }
}

impl<W: TextWrite> RenderNode<W> for AdmonitionHtmlRenderer<W> {
    fn render_node<'a>(
        &self,
        w: &mut W,
        _source: &'a str,
        _arena: &'a Arena,
        _node_ref: NodeRef,
        entering: bool,
        _context: &mut renderer::Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<div class=\"")?;
            self.writer
                .write_html(w, self.options.class_name.as_str())?;
            self.writer.write_safe_str(w, "\">")?;
        } else {
            self.writer.write_safe_str(w, "</div>")?;
        }
        Ok(WalkStatus::Continue)
    }
}

impl<'cb, W> NodeRenderer<'cb, W> for AdmonitionHtmlRenderer<W>
where
    W: TextWrite + 'cb,
{
    fn register_node_renderer_fn(self, nrr: &mut impl NodeRendererRegistry<'cb, W>) {
        nrr.register_node_renderer_fn(TypeId::of::<Admonition>(), BoxRenderNode::new(self));
    }
}

fn adomonition_parser_extension(opts: impl Into<AdmonitionParserOptions>) -> impl ParserExtension {
    ParserExtensionFn::new(|p: &mut Parser| {
        p.add_ast_transformer(AdmonitionAstTransformer::with_options, opts.into(), 100);
    })
}

fn adomonition_html_renderer_extension<'cb, W>(
    options: impl Into<AdmonitionHtmlRendererOptions>,
) -> impl RendererExtension<'cb, W>
where
    W: TextWrite + 'cb,
{
    RendererExtensionFn::new(move |r: &mut Renderer<'cb, W>| {
        r.add_node_renderer(AdmonitionHtmlRenderer::with_options, options.into());
    })
}

#[derive(Debug, Clone)]
struct HeaderFooterHtmlRendererOptions {
    pub header: String,
    pub footer: String,
}

impl RendererOptions for HeaderFooterHtmlRendererOptions {}

impl Default for HeaderFooterHtmlRendererOptions {
    fn default() -> Self {
        Self {
            header: "<div class=\"header\">Header</div>".to_string(),
            footer: "<div class=\"footer\">Footer</div>".to_string(),
        }
    }
}

struct HeaderPreRenderHook<W: TextWrite> {
    _phantom: core::marker::PhantomData<W>,
    writer: html::Writer,
    options: HeaderFooterHtmlRendererOptions,
}

impl<W: TextWrite> HeaderPreRenderHook<W> {
    fn with_options(html_opts: Options, options: HeaderFooterHtmlRendererOptions) -> Self {
        Self {
            _phantom: core::marker::PhantomData,
            writer: html::Writer::with_options(html_opts),
            options,
        }
    }
}

impl<W: TextWrite> PreRender<W> for HeaderPreRenderHook<W> {
    fn pre_render(
        &self,
        w: &mut W,
        _source: &str,
        _arena: &Arena,
        _node_ref: NodeRef,
        _context: &mut renderer::Context,
    ) -> Result<()> {
        self.writer.write_html(w, self.options.header.as_str())?;
        Ok(())
    }
}

struct FooterPostRenderHook<W: TextWrite> {
    _phantom: core::marker::PhantomData<W>,
    writer: html::Writer,
    options: HeaderFooterHtmlRendererOptions,
}

impl<W: TextWrite> FooterPostRenderHook<W> {
    fn with_options(html_opts: Options, options: HeaderFooterHtmlRendererOptions) -> Self {
        Self {
            _phantom: core::marker::PhantomData,
            writer: html::Writer::with_options(html_opts),
            options,
        }
    }
}

impl<W: TextWrite> PostRender<W> for FooterPostRenderHook<W> {
    fn post_render(
        &self,
        w: &mut W,
        _source: &str,
        _arena: &Arena,
        _node_ref: NodeRef,
        _context: &mut renderer::Context,
    ) -> Result<()> {
        self.writer.write_html(w, self.options.footer.as_str())?;
        Ok(())
    }
}

fn header_footer_html_renderer_extension<'cb, W>(
    options: impl Into<HeaderFooterHtmlRendererOptions>,
) -> impl RendererExtension<'cb, W>
where
    W: TextWrite + 'cb,
{
    RendererExtensionFn::new(move |r: &mut Renderer<'cb, W>| {
        let options = options.into();
        r.add_pre_render_hook(HeaderPreRenderHook::with_options, options.clone(), 0);
        r.add_post_render_hook(
            FooterPostRenderHook::with_options,
            options.clone(),
            u32::MAX,
        );
    })
}

// }}}

#[test]
fn test_extension() {
    let input = r#"
Hello @alice!

> [!NOTE]
>
> **this is a note**
"#;

    let mut output = String::new();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options::default(),
        user_mention_parser_extension().and(adomonition_parser_extension(
            AdmonitionParserOptions::default(),
        )),
        user_mention_html_renderer_extension(UserMentionOptions {
            class_name: "user-mention".to_string(),
        })
        .and(adomonition_html_renderer_extension(
            AdmonitionHtmlRendererOptions::default(),
        ))
        .and(header_footer_html_renderer_extension(
            HeaderFooterHtmlRendererOptions {
                header: "<div class=\"header\">Header</div>".to_string(),
                footer: "<div class=\"footer\">Footer</div>".to_string(),
            },
        )),
    );
    match markdown_to_html(&mut output, input) {
        Ok(_) => {}
        Err(e) => {
            println!("Rendering error: {:?}", e);
        }
    }

    assert_eq!(
        output.trim(),
        r#"<div class="header">Header</div><p>Hello <span class="user-mention"><strong>alice</strong></span>!</p>
<div class="markdown-alert"><p>NOTE</p>
<p><strong>this is a note</strong></p>
</div><div class="footer">Footer</div>"#
    );
}
