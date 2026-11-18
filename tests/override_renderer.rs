extern crate alloc;

use alloc::string::String;
use core::any::TypeId;

#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use rushdown::println;
use rushdown::{
    ast::*,
    new_markdown_to_html, parser, renderer,
    renderer::{
        html::{renderer_extension, RendererExtension},
        *,
    },
};

#[allow(dead_code)]
struct CustomParagraphRenderer<W: TextWrite> {
    _phantom: core::marker::PhantomData<*const W>,
}

impl<W: TextWrite> CustomParagraphRenderer<W> {
    fn with_html_options(_options: html::Options) -> Self {
        Self {
            _phantom: core::marker::PhantomData,
        }
    }
}

impl<W: TextWrite> RenderNode<W> for CustomParagraphRenderer<W> {
    /// Renders a paragraph node.
    fn render_node<'a>(
        &self,
        w: &mut W,
        _source: &'a str,
        _arena: &'a Arena,
        _node_ref: NodeRef,
        entering: bool,
        _context: &mut renderer::Context,
    ) -> Result<WalkStatus, rushdown::Error> {
        if entering {
            w.write_str("<pp>")?;
        } else {
            w.write_str("</pp>")?;
        }
        Ok(WalkStatus::Continue)
    }
}

impl<'r, W> NodeRenderer<'r, W> for CustomParagraphRenderer<W>
where
    W: TextWrite + 'r,
{
    fn register_node_renderer_fn(self, nrr: &mut impl NodeRendererRegistry<'r, W>) {
        nrr.register_node_renderer_fn(TypeId::of::<Paragraph>(), BoxRenderNode::new(self));
    }
}

fn custom_paragraph_renderer<'r, W>() -> impl RendererExtension<'r, W>
where
    W: TextWrite + 'r,
{
    renderer_extension(|r| {
        r.add_node_renderer(
            CustomParagraphRenderer::with_html_options,
            NoRendererOptions,
        )
    })
}

#[test]
fn test_override_renderer() {
    let input = r#"
paragraph
"#;
    let mut output = String::new();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options::default(),
        parser::NO_EXTENSIONS,
        custom_paragraph_renderer(),
    );
    match markdown_to_html(&mut output, input) {
        Ok(_) => {}
        Err(e) => {
            println!("Rendering error: {:?}", e);
        }
    }
    assert_eq!(output, "<pp>paragraph</pp>");
}

struct CommentDecorator {
    label: &'static str,
}

impl<'r, W: TextWrite + 'r> NodeRenderer<'r, W> for CommentDecorator {
    fn register_node_renderer_fn(self, nrr: &mut impl NodeRendererRegistry<'r, W>) {
        let label = self.label;
        nrr.register_node_renderer_decorator_fn(
            TypeId::of::<Strong>(),
            BoxRenderNodeDecorator::new(
                move |next: &NextRenderNode<'_, W>,
                      w: &mut W,
                      source: &str,
                      arena: &Arena,
                      node_ref: NodeRef,
                      entering: bool,
                      ctx: &mut renderer::Context| {
                    if entering {
                        w.write_str("<!--")?;
                        w.write_str(label)?;
                        w.write_str("-->")?;
                    }
                    next.render_node(w, source, arena, node_ref, entering, ctx)
                },
            ),
        );
    }
}

fn comment_decorator<'r, W: TextWrite + 'r>(label: &'static str) -> impl RendererExtension<'r, W> {
    renderer_extension(move |r| {
        r.add_node_renderer(move || CommentDecorator { label }, NoRendererOptions)
    })
}

#[test]
fn test_node_renderer_decorator() {
    let mut output = String::new();
    let markdown_to_html = new_markdown_to_html(
        parser::Options::default(),
        html::Options::default(),
        parser::NO_EXTENSIONS,
        comment_decorator("a")
            .and(comment_decorator("b"))
            .and(custom_paragraph_renderer()),
    );
    markdown_to_html(&mut output, "**important**\n").unwrap();
    assert_eq!(
        output,
        "<pp><!--b--><!--a--><strong>important</strong></pp>"
    );
}
