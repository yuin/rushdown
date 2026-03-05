//! HTML renderer for the AST.

#[cfg(not(feature = "std"))]
extern crate alloc;

use alloc::string::{String, ToString};

use crate::error::Error;
use crate::renderer::BuiltinNodesRenderer as _;
use crate::renderer::{self, *};
use crate::util::{
    escape_html, escape_url, has_suffix, try_escape_html_byte, try_resolve_entity_reference,
    try_resolve_numeric_reference, try_unescape_punct, AsciiWordSet, EscapeUrlOptions,
    UnescapePunctResult,
};
use crate::{as_kind_data, as_type_data, matches_kind};

// FormatOptions {{{

/// Common options for rendering HTML.
#[derive(Debug, Clone)]
pub struct Options {
    /// Renders soft line breaks as hard line breaks (`<br />`).
    pub hard_wraps: bool,

    /// Renders as XHTML.
    pub xhtml: bool,

    /// Allows raw HTML and unsafe links.
    pub allows_unsafe: bool,

    /// Indicates that a '\' escaped half-space(0x20) should not be rendered.
    pub escaped_space: bool,

    /// Attribute filters for rendering.
    pub attribute_filters: Option<Rc<AttributeFilters>>,
}

impl Default for Options {
    /// Creates default options for HTML rendering.
    fn default() -> Self {
        Self {
            hard_wraps: false,
            xhtml: false,
            allows_unsafe: false,
            escaped_space: false,
            attribute_filters: Some(Rc::new(AttributeFilters::default())),
        }
    }
}

impl FormatOptions for Options {}

impl<T: RendererOptions> FromRendererConstructorOptions<Options, T> for Options {
    fn from_renderer_constructor_options(opt: &RendererConstructorOptions<Options, T>) -> Self {
        opt.format_options.clone()
    }
}

// }}} FormatOptions

// AttributeFilters {{{

/// Filters for attributes when rendering.
#[derive(Debug)]
pub struct AttributeFilters {
    // blocks
    paragraph: Option<Rc<AsciiWordSet>>,
    blockquote: Option<Rc<AsciiWordSet>>,
    heading: Option<Rc<AsciiWordSet>>,
    code_block: Option<Rc<AsciiWordSet>>,
    thematic_break: Option<Rc<AsciiWordSet>>,
    list: Option<Rc<AsciiWordSet>>,
    list_item: Option<Rc<AsciiWordSet>>,

    table: Option<Rc<AsciiWordSet>>,
    table_header: Option<Rc<AsciiWordSet>>,
    table_row: Option<Rc<AsciiWordSet>>,
    table_cell: Option<Rc<AsciiWordSet>>,

    // inlines
    code_span: Option<Rc<AsciiWordSet>>,
    link: Option<Rc<AsciiWordSet>>,
    image: Option<Rc<AsciiWordSet>>,
}

include!(concat!(env!("OUT_DIR"), "/html_attributes.rs"));

impl Default for AttributeFilters {
    /// Creates default attribute filters.
    fn default() -> Self {
        let default_attr_filter = Rc::new(AsciiWordSet::new(DEFAULT_ATTRS));
        let blockquote_attr_filter = Rc::new(AsciiWordSet::new(BLOCKQUOTE_ATTRS));
        let thematic_break_attr_filter = Rc::new(AsciiWordSet::new(THEMATIC_BREAK_ATTRS));
        let list_attr_filter = Rc::new(AsciiWordSet::new(LIST_ATTRS));
        let list_item_attr_filter = Rc::new(AsciiWordSet::new(LIST_ITEM_ATTRS));

        let table_attr_filter = Rc::new(AsciiWordSet::new(TABLE_ATTRS));
        let table_header_attr_filter = Rc::new(AsciiWordSet::new(TABLE_HEADER_ATTRS));
        let table_row_attr_filter = Rc::new(AsciiWordSet::new(TABLE_ROW_ATTRS));
        let table_cell_attr_filter = Rc::new(AsciiWordSet::new(TABLE_CELL_ATTRS));

        let link_attr_filter = Rc::new(AsciiWordSet::new(LINK_ATTRS));
        let image_attr_filter = Rc::new(AsciiWordSet::new(IMAGE_ATTRS));

        Self {
            paragraph: Some(default_attr_filter.clone()),
            blockquote: Some(blockquote_attr_filter.clone()),
            heading: Some(default_attr_filter.clone()),
            code_block: Some(default_attr_filter.clone()),
            thematic_break: Some(thematic_break_attr_filter.clone()),
            list: Some(list_attr_filter.clone()),
            list_item: Some(list_item_attr_filter.clone()),

            table: Some(table_attr_filter.clone()),
            table_header: Some(table_header_attr_filter.clone()),
            table_row: Some(table_row_attr_filter.clone()),
            table_cell: Some(table_cell_attr_filter.clone()),

            code_span: Some(default_attr_filter.clone()),
            link: Some(link_attr_filter.clone()),
            image: Some(image_attr_filter.clone()),
        }
    }
}

macro_rules! impl_attribute_filter {
    ($setter:ident, $field:ident) => {
        /// Returns the valid attribute names for $field nodes.
        pub fn $field(&self) -> Option<&AsciiWordSet> {
            self.$field.as_deref()
        }

        /// Sets the valid attribute names for $field nodes.
        pub fn $setter(&mut self, attrs: AsciiWordSet) {
            self.$field = Some(Rc::new(attrs));
        }
    };
}

impl AttributeFilters {
    /// Creates a new empty `AttributeFilters`.
    pub fn new() -> Self {
        Self::default()
    }
    impl_attribute_filter!(set_paragraph, paragraph);
    impl_attribute_filter!(set_blockquote, blockquote);
    impl_attribute_filter!(set_heading, heading);
    impl_attribute_filter!(set_code_block, code_block);
    impl_attribute_filter!(set_thematic_break, thematic_break);
    impl_attribute_filter!(set_list, list);
    impl_attribute_filter!(set_list_item, list_item);

    impl_attribute_filter!(set_table, table);
    impl_attribute_filter!(set_table_header, table_header);
    impl_attribute_filter!(set_table_row, table_row);
    impl_attribute_filter!(set_table_cell, table_cell);

    impl_attribute_filter!(set_code_span, code_span);
    impl_attribute_filter!(set_link, link);
    impl_attribute_filter!(set_image, image);
}

// }}} AttributeFilters

// BuiltinNodesRenderer {{{

macro_rules! attribute_filter_for {
    ($format_options:expr, $field:ident) => {
        if let Some(filters) = &$format_options.attribute_filters {
            if let Some(filter) = filters.$field() {
                Some(filter)
            } else {
                None
            }
        } else {
            None
        }
    };
}

macro_rules! write_attributes {
    ($arena:expr, $node_ref:expr, $source:expr, $writer:expr, $format_options:expr, $field:ident) => {
        let attrs = $arena[$node_ref].attributes();
        if !attrs.is_empty() {
            render_attributes(
                $writer,
                $source,
                attrs,
                attribute_filter_for!($format_options, $field),
            )?;
        }
    };
}

/// Default implementation of [`super::BuiltinNodesRenderer`] for HTML output.
#[derive(Debug)]
pub struct BuiltinNodesRenderer<W: TextWrite = String> {
    format_options: Options,
    writer: Writer,
    _phantom: core::marker::PhantomData<W>,
}

impl<W: TextWrite> BuiltinNodesRenderer<W> {
    /// Create a new [`BuiltinNodesRenderer`] with the given options.
    pub fn new(format_options: Options) -> Self {
        Self {
            format_options: format_options.clone(),
            writer: Writer::with_options(format_options),
            _phantom: core::marker::PhantomData,
        }
    }

    fn render_texts<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        context: &mut Context,
    ) -> Result<()> {
        for c in arena[node_ref].children(arena) {
            if matches_kind!(arena[c], Text) {
                self.render_text(w, source, arena, c, true, context)?;
            } else {
                self.render_texts(w, source, arena, c, context)?;
            }
        }
        Ok(())
    }
}

impl<W: TextWrite> renderer::BuiltinNodesRenderer<W> for BuiltinNodesRenderer<W> {
    fn render_document<'a>(
        &self,
        _write: &mut W,
        _source: &'a str,
        _arena: &'a ast::Arena,
        _node_ref: ast::NodeRef,
        _entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        Ok(WalkStatus::Continue)
    }

    fn render_paragraph<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            let should_open = !is_in_tight_list(arena, node_ref);
            if should_open {
                self.writer.write_safe_str(w, "<p")?;
                write_attributes!(arena, node_ref, source, w, self.format_options, paragraph);
                self.writer.write_safe_str(w, ">")?;
            }
            if let Some(task) = context.pop_task() {
                match task {
                    Task::Checked => {
                        self.writer.write_safe_str(
                            w,
                            r#"<input checked="" disabled="" type="checkbox""#,
                        )?;
                    }
                    Task::Unchecked => {
                        self.writer
                            .write_safe_str(w, r#"<input disabled="" type="checkbox""#)?;
                    }
                }
                if self.format_options.xhtml {
                    self.writer.write_safe_str(w, " /> ")?;
                } else {
                    self.writer.write_safe_str(w, "> ")?;
                }
            }
        } else {
            let opened = !is_in_tight_list(arena, node_ref);
            if !opened {
                let n = &arena[node_ref];
                if n.next_sibling().is_some() && n.first_child().is_some() {
                    self.writer.write_newline(w)?;
                }
            } else {
                self.writer.write_safe_str(w, "</p>\n")?;
            }
        }
        Ok(WalkStatus::Continue)
    }

    fn render_heading<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if let KindData::Heading(heading) = arena[node_ref].kind_data() {
            if entering {
                self.writer.write_safe_str(w, "<h")?;
                self.writer
                    .write_safe_str(w, SafeBytes(&[b'0' + heading.level()]))?;
                write_attributes!(arena, node_ref, source, w, self.format_options, heading);
                self.writer.write_safe_str(w, ">")?;
            } else {
                self.writer.write_safe_str(w, "</h")?;
                self.writer
                    .write_safe_str(w, SafeBytes(&[b'0' + heading.level()]))?;
                self.writer.write_safe_str(w, ">\n")?;
            }
        }
        Ok(WalkStatus::Continue)
    }

    fn render_thematic_break<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if !entering {
            return Ok(WalkStatus::Continue);
        }
        if entering {
            self.writer.write_safe_str(w, "<hr")?;
            write_attributes!(
                arena,
                node_ref,
                source,
                w,
                self.format_options,
                thematic_break
            );
            if self.format_options.xhtml {
                self.writer.write_safe_str(w, " />\n")?;
            } else {
                self.writer.write_safe_str(w, ">\n")?;
            }
        }
        Ok(WalkStatus::Continue)
    }

    fn render_code_block<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<pre><code")?;
            let kd = as_kind_data!(arena, node_ref, CodeBlock);
            if let Some(lang) = kd.language(source) {
                self.writer.write_safe_str(w, " class=\"language-")?;
                self.writer.write(w, lang)?;
                self.writer.write_safe_str(w, "\"")?;
            }
            self.writer.write_safe_str(w, ">")?;
            let block = as_type_data!(arena, node_ref, Block);
            for line in block.lines().iter() {
                self.writer.raw_write(w, &line.str(source))?;
            }
        } else {
            self.writer.write_safe_str(w, "</code></pre>\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_blockquote<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<blockquote")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, blockquote);
            self.writer.write_safe_str(w, ">\n")?;
        } else {
            self.writer.write_safe_str(w, "</blockquote>\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_list<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        let node = as_kind_data!(arena, node_ref, List);
        let tag = if node.is_ordered() { "ol" } else { "ul" };
        if entering {
            self.writer.write_safe_str(w, "<")?;
            self.writer.write_safe_str(w, tag)?;
            if node.is_ordered() && node.start() != 1 {
                self.writer.write_safe_str(w, " start=\"")?;
                let start = node.start().to_string();
                self.writer.write_safe_str(w, SafeBytes(start.as_bytes()))?;
                self.writer.write_safe_str(w, "\"")?;
            }
            write_attributes!(arena, node_ref, source, w, self.format_options, list);
            self.writer.write_safe_str(w, ">\n")?;
        } else {
            self.writer.write_safe_str(w, "</")?;
            self.writer.write_safe_str(w, tag)?;
            self.writer.write_safe_str(w, ">\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_list_item<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<li")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, list_item);
            self.writer.write_safe_str(w, ">")?;
            if let Some(p) = arena[node_ref].parent() {
                if let KindData::List(list) = arena[p].kind_data() {
                    if let Some(first_child) = arena[node_ref].first_child() {
                        if !list.is_tight() || !matches_kind!(arena, first_child, Paragraph) {
                            self.writer.write_newline(w)?;
                        }
                    }
                }
            }
            context.set_task(as_kind_data!(arena, node_ref, ListItem).task());
        } else {
            self.writer.write_safe_str(w, "</li>\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_html_block<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            if self.format_options.allows_unsafe {
                let block = as_type_data!(arena, node_ref, Block);
                for line in block.lines().iter() {
                    self.writer.write_html(w, &line.str(source))?;
                }
            } else {
                self.writer
                    .write_safe_str(w, "<!-- raw HTML omitted -->\n")?;
            }
        }
        Ok(WalkStatus::Continue)
    }

    fn render_table<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<table")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, table);
            self.writer.write_safe_str(w, ">\n")?;
        } else {
            self.writer.write_safe_str(w, "</table>\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_table_header<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<thead")?;
            write_attributes!(
                arena,
                node_ref,
                source,
                w,
                self.format_options,
                table_header
            );
            self.writer.write_safe_str(w, ">\n")?;
        } else {
            self.writer.write_safe_str(w, "</thead>\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_table_body<'a>(
        &self,
        w: &mut W,
        _source: &'a str,
        _arena: &'a ast::Arena,
        _node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<tbody>\n")?;
        } else {
            self.writer.write_safe_str(w, "</tbody>\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_table_row<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<tr")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, table_row);
            self.writer.write_safe_str(w, ">\n")?;
        } else {
            self.writer.write_safe_str(w, "</tr>\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_table_cell<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        let tag = if arena[node_ref].parent().is_some_and(|p| {
            arena[p]
                .parent()
                .is_some_and(|gp| matches_kind!(arena, gp, TableHeader))
        }) {
            "th"
        } else {
            "td"
        };

        if entering {
            self.writer.write_safe_str(w, "<")?;
            self.writer.write_safe_str(w, SafeBytes(tag.as_bytes()))?;
            if self.format_options.xhtml {
                match as_kind_data!(arena, node_ref, TableCell).alignment() {
                    TableCellAlignment::None => {}
                    n => {
                        self.writer.write_safe_str(w, r#" align=""#)?;
                        self.writer.write(w, n.as_str())?;
                        self.writer.write_safe_str(w, r#"""#)?;
                    }
                }
            } else {
                match as_kind_data!(arena, node_ref, TableCell).alignment() {
                    TableCellAlignment::None => {}
                    n => {
                        self.writer.write_safe_str(w, r#" style="text-align: "#)?;
                        self.writer.write(w, n.as_str())?;
                        self.writer.write_safe_str(w, r#";""#)?;
                    }
                }
            }
            write_attributes!(arena, node_ref, source, w, self.format_options, table_cell);
            self.writer.write_safe_str(w, ">")?;
        } else {
            self.writer.write_safe_str(w, "</")?;
            self.writer.write_safe_str(w, SafeBytes(tag.as_bytes()))?;
            self.writer.write_safe_str(w, ">\n")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_text<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            let kd = as_kind_data!(arena, node_ref, Text);
            if kd.has_qualifiers(TextQualifier::CODE) {
                self.writer.write_html(w, &kd.str(source))?;
            } else if kd.has_qualifiers(TextQualifier::RAW) {
                self.writer.raw_write(w, &kd.str(source))?;
            } else {
                self.writer.write(w, &kd.str(source))?;
                if kd.has_qualifiers(TextQualifier::HARD_LINE_BREAK)
                    || (kd.has_qualifiers(TextQualifier::SOFT_LINE_BREAK)
                        && self.format_options.hard_wraps)
                {
                    if self.format_options.xhtml {
                        self.writer.write_safe_str(w, "<br />\n")?;
                    } else {
                        self.writer.write_safe_str(w, "<br>\n")?;
                    }
                } else if kd.has_qualifiers(TextQualifier::SOFT_LINE_BREAK) {
                    self.writer.write_newline(w)?;
                }
            }
        }
        Ok(WalkStatus::Continue)
    }

    fn render_code_span<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<code")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, code_span);
            self.writer.write_safe_str(w, ">")?;
            for c in arena[node_ref].children(arena) {
                let tkd = as_kind_data!(arena, c, Text);
                let value = &tkd.bytes(source);
                if has_suffix(value, b"\n") {
                    self.writer.raw_write(w, unsafe {
                        str::from_utf8_unchecked(&value[..value.len() - 1])
                    })?;
                    self.writer.write_safe_str(w, " ")?;
                } else {
                    self.writer
                        .raw_write(w, unsafe { str::from_utf8_unchecked(value) })?;
                }
            }
            return Ok(WalkStatus::SkipChildren);
        } else {
            self.writer.write_safe_str(w, "</code>")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_emphasis<'a>(
        &self,
        w: &mut W,
        _source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        let kd = as_kind_data!(arena, node_ref, Emphasis);
        let tag = if kd.level() == 1 { "em" } else { "strong" };
        if entering {
            self.writer.write_safe_str(w, "<")?;
            self.writer.write_safe_str(w, tag)?;
            self.writer.write_safe_str(w, ">")?;
        } else {
            self.writer.write_safe_str(w, "</")?;
            self.writer.write_safe_str(w, tag)?;
            self.writer.write_safe_str(w, ">")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_link<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            let kd = as_kind_data!(arena, node_ref, Link);
            let dest = kd.destination().str(source);
            self.writer.write_safe_str(w, "<a href=\"")?;
            if self.format_options.allows_unsafe || !is_dangerous_url(dest.as_bytes()) {
                let mut u = escape_url(
                    dest.as_bytes(),
                    &EscapeUrlOptions {
                        resolves_refs: !kd.is_auto_link(),
                        ..EscapeUrlOptions::for_url()
                    },
                );
                u = escape_html(u);
                self.writer.write_safe_str(w, SafeBytes(&u))?;
            }
            self.writer.write_safe_str(w, "\"")?;
            if let Some(title) = kd.title() {
                self.writer.write_safe_str(w, " title=\"")?;
                self.writer.write(w, title.str(source))?;
                self.writer.write_safe_str(w, "\"")?;
            }
            write_attributes!(arena, node_ref, source, w, self.format_options, link);
            self.writer.write_safe_str(w, ">")?;
        } else {
            self.writer.write_safe_str(w, "</a>")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_image<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        ctx: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            let kd = as_kind_data!(arena, node_ref, Image);
            let dest = kd.destination().str(source);
            self.writer.write_safe_str(w, "<img src=\"")?;
            if self.format_options.allows_unsafe || !is_dangerous_url(dest.as_bytes()) {
                w.write_str(dest)?;
            }
            self.writer.write_safe_str(w, "\" alt=\"")?;
            self.render_texts(w, source, arena, node_ref, ctx)?;
            self.writer.write_safe_str(w, "\"")?;
            if let Some(title) = kd.title() {
                self.writer.write_safe_str(w, " title=\"")?;
                self.writer.write(w, title.str(source))?;
                self.writer.write_safe_str(w, "\"")?;
            }
            write_attributes!(arena, node_ref, source, w, self.format_options, image);
            if self.format_options.xhtml {
                w.write_str(" />")?;
            } else {
                w.write_str(">")?;
            }
        }
        Ok(WalkStatus::SkipChildren)
    }

    fn render_raw_html<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a ast::Arena,
        node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            let kd = as_kind_data!(arena, node_ref, RawHtml);
            if self.format_options.allows_unsafe {
                for line in kd.lines().iter() {
                    self.writer.write_html(w, &line.str(source))?;
                }
            } else {
                self.writer.write_safe_str(w, "<!-- raw HTML omitted -->")?;
            }
        }
        Ok(WalkStatus::Continue)
    }

    fn render_strikethrough<'a>(
        &self,
        w: &mut W,
        _source: &'a str,
        _arena: &'a ast::Arena,
        _node_ref: ast::NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            self.writer.write_safe_str(w, "<del>")?;
        } else {
            self.writer.write_safe_str(w, "</del>")?;
        }
        Ok(WalkStatus::Continue)
    }
}

// }}} BuiltinNodesRenderer

// Renderer {{{

/// A trait for extending the renderer with custom behavior.
pub trait RendererExtension<'r, W: TextWrite = String> {
    /// Applies the extension to the given renderer.
    fn apply(self, renderer: &mut Renderer<'r, W>);

    /// Chains this extension with another extension.
    fn and<R>(self, other: R) -> ChainedRendererExtension<Self, R>
    where
        Self: Sized,
        R: RendererExtension<'r, W>,
    {
        ChainedRendererExtension {
            first: self,
            second: other,
        }
    }
}

/// An empty renderer extension that does nothing.
#[derive(Debug, Default)]
pub struct EmptyRendererExtension;

impl EmptyRendererExtension {
    pub fn new() -> Self {
        Self {}
    }
}

impl<'r, W: TextWrite> RendererExtension<'r, W> for EmptyRendererExtension {
    fn apply(self, _renderer: &mut Renderer<'r, W>) {}
}

/// A constant for an empty renderer extension.
pub const NO_EXTENSIONS: EmptyRendererExtension = EmptyRendererExtension;

/// A renderer extension that chains two extensions together.
pub struct ChainedRendererExtension<T, U> {
    pub first: T,
    pub second: U,
}

impl<'r, T, U, W> RendererExtension<'r, W> for ChainedRendererExtension<T, U>
where
    W: TextWrite + 'r,
    T: RendererExtension<'r, W>,
    U: RendererExtension<'r, W>,
{
    fn apply(self, renderer: &mut Renderer<'r, W>) {
        self.first.apply(renderer);
        self.second.apply(renderer);
    }
}

/// A renderer extension defined by a closure.
pub struct RendererExtensionFn<T> {
    f: T,
}

impl<T> RendererExtensionFn<T> {
    /// Creates a new `RendererExtensionFn`.
    pub fn new(f: T) -> Self {
        Self { f }
    }
}

impl<'r, T, W> RendererExtension<'r, W> for RendererExtensionFn<T>
where
    W: TextWrite + 'r,
    T: FnOnce(&mut Renderer<'r, W>),
{
    fn apply(self, renderer: &mut Renderer<'r, W>) {
        (self.f)(renderer);
    }
}

impl<T> From<T> for RendererExtensionFn<T> {
    fn from(f: T) -> Self {
        RendererExtensionFn { f }
    }
}

/// Renderer for HTML output.
pub struct Renderer<'r, W: TextWrite = String> {
    helper: RendererHelper<'r, W, BuiltinNodesRenderer<W>, Options>,
}

impl<'r, W: TextWrite> Default for Renderer<'r, W> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'r, W: TextWrite> Renderer<'r, W> {
    /// Create a new `Renderer` with default options.
    pub fn new() -> Self {
        Self::with_options(Options::default())
    }

    /// Create a new `Renderer` with the given options.
    pub fn with_options(options: Options) -> Self {
        Self::with_extensions(options, EmptyRendererExtension::new())
    }

    /// Create a new `Renderer` with the given options and extensions.
    ///
    /// - [`add_node_renderer`] can only be called within extensions.
    ///
    /// [`add_node_renderer`]: Self::add_node_renderer
    pub fn with_extensions(options: Options, ext: impl RendererExtension<'r, W>) -> Self {
        let helper = RendererHelper::new(options.clone(), BuiltinNodesRenderer::new(options));
        let mut s = Self { helper };
        ext.apply(&mut s);
        s
    }

    /// Add a custom node renderer.
    pub fn add_node_renderer<A, T, R, F>(&mut self, f: F, ropt: T)
    where
        T: RendererOptions,
        F: RendererConstructor<A, Options, T, R>,
        R: NodeRenderer<'r, W>,
    {
        self.helper.add_node_renderer(f, ropt);
    }

    /// Render the AST to the given writer.
    pub fn render<'a>(
        &self,
        writer: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
    ) -> Result<()> {
        self.helper.render(writer, source, arena, node_ref)
    }
}

// }}} Renderer

// Writer {{{
/// A writer for HTML output.
/// All HTML renderer should use this for writing HTML content for proper escaping.
#[derive(Debug, Default)]
pub struct Writer {
    options: Options,
}

const REPLACEMENT_CHAR: char = '\u{FFFD}';

impl Writer {
    /// Creates a new [`Writer`] with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new [`Writer`] with the given HTML options.
    pub fn with_options(options: Options) -> Self {
        Self { options }
    }

    /// Writes the given `s` to `w` with:
    ///
    /// - resolving references and unescaping
    /// - backslash escaped characters
    /// - replacing insecure HTML characters(null characters)
    /// - replacing HTML entities(`<`, `>`, `&`, `"`, `'`, and so on)
    pub fn write<W: TextWrite>(&self, w: &mut W, s: &str) -> Result<()> {
        let bytes = s.as_bytes();
        let len = s.len();
        let mut i = 0usize;
        let mut n = 0usize;
        while i < len {
            let c = bytes[i];
            if c == b'\\' {
                match try_unescape_punct(bytes, i, self.options.escaped_space) {
                    UnescapePunctResult::Punct(nbyte, ch) => {
                        self.raw_write(w, &s[n..i])?;
                        if let Some(esc) = try_escape_html_byte(ch) {
                            w.write_str(esc)?;
                        } else {
                            w.write_char(ch as char)?;
                        }
                        i = i + nbyte + 1;
                        n = i;
                        continue;
                    }
                    UnescapePunctResult::Skipped(nbyte) => {
                        self.raw_write(w, &s[n..i])?;
                        i = i + nbyte + 1;
                        n = i;
                        continue;
                    }
                    UnescapePunctResult::None => {}
                }
            }
            if c == b'&' {
                if let Some((nbyte, ch)) = try_resolve_numeric_reference(bytes, i) {
                    self.raw_write(w, &s[n..i])?;
                    let mut buf = [0u8; 4];
                    let s: &str = ch.encode_utf8(&mut buf);
                    self.raw_write(w, s)?;
                    i = i + nbyte + 1;
                    n = i;
                    continue;
                }
                if let Some((nbyte, ch)) = try_resolve_entity_reference(bytes, i) {
                    self.raw_write(w, &s[n..i])?;
                    self.raw_write(w, ch)?;
                    i = i + nbyte + 1;
                    n = i;
                    continue;
                }
            }
            i += 1;
        }
        self.raw_write(w, &s[n..])
    }

    /// Writes the given `s` to `w` with:
    ///
    /// - replacing insecure HTML characters(null characters)
    /// - replacing HTML entities(`<`, `>`, `&`, `"`, `'`, and so on)
    pub fn raw_write<W: TextWrite>(&self, w: &mut W, s: &str) -> Result<()> {
        let bytes = s.as_bytes();
        let mut n = 0;

        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\0' {
                if i != n {
                    write_bytes(w, &bytes[n..i])?;
                }
                w.write_char(REPLACEMENT_CHAR)?;
                n = i + 1;
            } else if let Some(rep) = try_escape_html_byte(b) {
                if i != n {
                    write_bytes(w, &bytes[n..i])?;
                }
                w.write_str(rep)?;
                n = i + 1;
            }
        }

        if n != bytes.len() {
            write_bytes(w, &bytes[n..])?;
        }

        Ok(())
    }

    /// Writes the given `s` to `w` with:
    ///
    /// - replacing insecure HTML characters(null characters)
    pub fn write_html<W: TextWrite>(&self, w: &mut W, s: &str) -> Result<()> {
        let bytes = s.as_bytes();
        let mut i = 0;

        while let Some(rel) = memchr::memchr(0, &bytes[i..]) {
            let j = i + rel;
            if j != i {
                write_bytes(w, &bytes[i..j])?;
            }
            w.write_char(REPLACEMENT_CHAR)?;
            i = j + 1;
        }

        if i != bytes.len() {
            write_bytes(w, &bytes[i..])?;
        }

        Ok(())
    }

    /// Writes the given `s` to `w`.
    /// This function does not perform any escaping or processing.
    /// So use this only for safe strings.
    #[inline(always)]
    pub fn write_safe_str<W: TextWrite, S: SafeStr>(&self, w: &mut W, s: S) -> Result<()> {
        w.write_str(s.as_str())
    }

    /// Writes a newline.
    #[inline(always)]
    pub fn write_newline<W: TextWrite>(&self, w: &mut W) -> Result<()> {
        w.write_char('\n')
    }
}

// }}} Writer

// Utilities {{{

/// Renders attributes to the given writer.
/// You can specify a set of valid attribute names to filter the attributes.
#[inline]
pub fn render_attributes<W: TextWrite>(
    w: &mut W,
    source: &str,
    attributes: &Metadata,
    valid: Option<&AsciiWordSet>,
) -> Result<()> {
    for (key, value) in attributes.iter() {
        if !key.starts_with("data-") && !key.starts_with("aria-") {
            if let Some(valid_set) = valid {
                if !valid_set.contains(key.as_str()) {
                    continue;
                }
            }
        }
        w.write_str(" ")?;
        w.write_str(key.as_str())?;
        w.write_str("=\"")?;
        let b = value.bytes(source);
        write_bytes(w, &escape_html(b))?;
        w.write_str("\"")?;
    }
    Ok(())
}

#[inline(always)]
fn is_in_tight_list(arena: &ast::Arena, node_ref: ast::NodeRef) -> bool {
    if let Some(p) = arena[node_ref].parent() {
        if let Some(gp) = arena[p].parent() {
            if let KindData::List(list) = arena[gp].kind_data() {
                return list.is_tight();
            }
        }
    }
    false
}

pub(crate) mod private {
    pub trait Sealed {}
}

/// Marker trait for safe strings.
pub trait SafeStr: private::Sealed {
    fn as_str(&self) -> &str;
}

impl private::Sealed for &'static str {}
impl SafeStr for &'static str {
    #[inline(always)]
    fn as_str(&self) -> &str {
        self
    }
}

pub(crate) struct SafeBytes<'a>(&'a [u8]); // only valid within this crate;

impl private::Sealed for SafeBytes<'_> {}
impl<'a> SafeStr for SafeBytes<'a> {
    #[inline(always)]
    fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(self.0) }
    }
}

#[inline]
fn write_bytes<W: TextWrite>(w: &mut W, bytes: &[u8]) -> Result<()> {
    unsafe {
        w.write_str(core::str::from_utf8_unchecked(bytes))
            .map_err(|e| Error::io("Failed to write bytes", Some(Box::new(e))))
    }
}

const B_DATA_IMAGE: &[u8] = b"data:image/";
const B_PNG: &[u8] = b"png;";
const B_GIF: &[u8] = b"gif;";
const B_JPEG: &[u8] = b"jpeg;";
const B_WEBP: &[u8] = b"webp;";
const B_SVG: &[u8] = b"svg+xml;";
const B_JS: &[u8] = b"javascript:";
const B_VB: &[u8] = b"vbscript:";
const B_FILE: &[u8] = b"file:";
const B_DATA: &[u8] = b"data:";

fn has_prefix_ignore_ascii_case(s: &[u8], prefix: &[u8]) -> bool {
    if s.len() < prefix.len() {
        return false;
    }
    s[..prefix.len()]
        .iter()
        .zip(prefix.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Returns true if the given URL seems potentially dangerous, otherwise false.
pub fn is_dangerous_url(url: &[u8]) -> bool {
    if has_prefix_ignore_ascii_case(url, B_DATA_IMAGE) && url.len() >= 11 {
        let v = &url[11..];
        if has_prefix_ignore_ascii_case(v, B_PNG)
            || has_prefix_ignore_ascii_case(v, B_GIF)
            || has_prefix_ignore_ascii_case(v, B_JPEG)
            || has_prefix_ignore_ascii_case(v, B_WEBP)
            || has_prefix_ignore_ascii_case(v, B_SVG)
        {
            return false;
        }
        return true;
    }

    has_prefix_ignore_ascii_case(url, B_JS)
        || has_prefix_ignore_ascii_case(url, B_VB)
        || has_prefix_ignore_ascii_case(url, B_FILE)
        || has_prefix_ignore_ascii_case(url, B_DATA)
}

// }}} Utilities

// ParagraphRenderer {{{

/// Options for the paragraph renderer.
#[derive(Default)]
pub struct ParagraphRendererOptions<W: TextWrite = String> {
    /// A renderer function for task list item checkboxes.
    /// NodeRef provided is the parent of the paragraph node (the task list item).
    #[allow(clippy::type_complexity)]
    pub render_task_list_item: Option<
        Box<
            dyn Fn(
                &mut W,
                &ParagraphRenderer<W>,
                &str,
                &ast::Arena,
                ast::NodeRef,
                &mut Context,
            ) -> Result<()>,
        >,
    >,
}

impl<W: TextWrite> RendererOptions for ParagraphRendererOptions<W> {}

/// A renderer for paragraph nodes.
pub struct ParagraphRenderer<W: TextWrite = String> {
    writer: html::Writer,
    format_options: Options,
    options: ParagraphRendererOptions<W>,
}

impl<W: TextWrite> ParagraphRenderer<W> {
    fn with_options(html_opts: Options, options: ParagraphRendererOptions<W>) -> Self {
        Self {
            writer: html::Writer::with_options(html_opts.clone()),
            format_options: html_opts,
            options,
        }
    }

    /// Returns a reference to the HTML writer used by this renderer.
    pub fn writer(&self) -> &Writer {
        &self.writer
    }

    /// Returns a reference to the HTML format options used by this renderer.
    pub fn format_options(&self) -> &Options {
        &self.format_options
    }

    /// Returns a reference to the paragraph renderer options used by this renderer.
    pub fn options(&self) -> &ParagraphRendererOptions<W> {
        &self.options
    }
}

impl<W: TextWrite> RenderNode<W> for ParagraphRenderer<W> {
    fn render_node<'a>(
        &self,
        w: &mut W,
        source: &'a str,
        arena: &'a Arena,
        node_ref: NodeRef,
        entering: bool,
        context: &mut renderer::Context,
    ) -> Result<WalkStatus> {
        if entering {
            let should_render = is_in_tight_list(arena, node_ref);
            if !should_render {
                self.writer.write_safe_str(w, "<p")?;
                write_attributes!(arena, node_ref, source, w, self.format_options, paragraph);
                self.writer.write_safe_str(w, ">")?;
            }
            if let Some(task) = context.pop_task() {
                if let Some(ref r) = self.options.render_task_list_item {
                    r(
                        w,
                        self,
                        source,
                        arena,
                        arena[node_ref].parent().unwrap(),
                        context,
                    )?;
                } else {
                    match task {
                        Task::Checked => {
                            self.writer.write_safe_str(
                                w,
                                r#"<input checked="" disabled="" type="checkbox""#,
                            )?;
                        }
                        Task::Unchecked => {
                            self.writer
                                .write_safe_str(w, r#"<input disabled="" type="checkbox""#)?;
                        }
                    }
                    if self.format_options.xhtml {
                        self.writer.write_safe_str(w, " /> ")?;
                    } else {
                        self.writer.write_safe_str(w, "> ")?;
                    }
                }
            }
        } else {
            let opened = !is_in_tight_list(arena, node_ref);
            if !opened {
                let n = &arena[node_ref];
                if n.next_sibling().is_some() && n.first_child().is_some() {
                    self.writer.write_newline(w)?;
                }
            } else {
                self.writer.write_safe_str(w, "</p>\n")?;
            }
        }
        Ok(WalkStatus::Continue)
    }
}

impl<'r, W> NodeRenderer<'r, W> for ParagraphRenderer<W>
where
    W: TextWrite + 'r,
{
    fn register_node_renderer_fn(self, nrr: &mut impl NodeRendererRegistry<'r, W>) {
        nrr.register_node_renderer_fn(TypeId::of::<Paragraph>(), BoxRenderNode::new(self));
    }
}

/// Creates a paragraph renderer with the given options.
pub fn paragraph_renderer<'r, W>(
    options: impl Into<ParagraphRendererOptions<W>>,
) -> impl RendererExtension<'r, W>
where
    W: TextWrite + 'r,
{
    RendererExtensionFn::new(move |r: &mut Renderer<'r, W>| {
        r.add_node_renderer(ParagraphRenderer::with_options, options.into());
    })
}

// }}} ParagraphRenderer
