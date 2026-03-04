extern crate alloc;

use alloc::string::String;
use rushdown::new_markdown_to_html;
use rushdown::parser;
use rushdown::renderer::html::Renderer;
use rushdown::renderer::html::RendererExtension;
use rushdown::renderer::html::RendererExtensionFn;

use core::any::TypeId;

use rushdown::ast::*;
use rushdown::renderer;
use rushdown::renderer::*;

#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use rushdown::println;

#[allow(dead_code)]
struct CustomParagraphRenderer<W: TextWrite> {
    _phantom: core::marker::PhantomData<W>,
    writer: html::Writer,
}

impl<W: TextWrite> CustomParagraphRenderer<W> {
    fn with_html_options(options: html::Options) -> Self {
        Self {
            _phantom: core::marker::PhantomData,
            writer: html::Writer::with_options(options),
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
            self.writer.write_safe_str(w, "<pp>")?;
        } else {
            self.writer.write_safe_str(w, "</pp>")?;
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
    RendererExtensionFn::new(move |r: &mut Renderer<'r, W>| {
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
    // let mut output = String::new();
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
