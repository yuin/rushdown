//! HTML renderer for the AST.

#[cfg(not(feature = "std"))]
extern crate alloc;

use alloc::string::String;

use memchr::memchr;

use crate::{
    as_kind_data,
    error::Error,
    matches_kind,
    renderer::{self, BuiltinNodesRenderer as _, *},
    util::{self, escape_html, escape_url, try_escape_html_byte, AsciiWordSet, EscapeUrlOptions},
};

// FormatOptions {{{

/// Common options for rendering HTML.
#[derive(Clone)]
pub struct Options {
    /// Renders soft line breaks as hard line breaks (`<br />`).
    pub hard_wraps: bool,

    /// Renders as XHTML.
    pub xhtml: bool,

    /// Allows raw HTML and unsafe links.
    pub allows_unsafe: bool,

    /// A strategy that decides whether soft line breaks are rendered as newlines.
    /// If [`None`], soft line breaks are always rendered as newlines.
    /// `hard_wraps` takes precedence over this option.
    pub line_break_strategy: Option<LineBreakStrategy>,

    /// Attribute filters for rendering.
    pub attribute_filters: Option<Rc<AttributeFilters>>,

    /// A function that initializes the [`ContextKeyRegistry`] when the renderer is created.
    pub context_key_registry_initializer: Option<Rc<dyn InitializeContextKeyRegistry>>,
}

impl core::fmt::Debug for Options {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Options")
            .field("hard_wraps", &self.hard_wraps)
            .field("xhtml", &self.xhtml)
            .field("allows_unsafe", &self.allows_unsafe)
            .field("line_break_strategy", &self.line_break_strategy)
            .field("attribute_filters", &self.attribute_filters)
            .field(
                "context_key_registry_initializer",
                &"Option<Rc<dyn InitializeContextKeyRegistry>>",
            )
            .finish()
    }
}

impl Default for Options {
    /// Creates default options for HTML rendering.
    fn default() -> Self {
        Self {
            hard_wraps: false,
            xhtml: false,
            allows_unsafe: false,
            line_break_strategy: None,
            attribute_filters: Some(Rc::new(AttributeFilters::default())),
            context_key_registry_initializer: None,
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

// LineBreakStrategy {{{

/// A function that decides whether a soft line break between two characters is rendered
/// as a newline. Arguments are the last character before and the first character after
/// the soft line break; [`None`] means that no such character exists.
pub type SoftLineBreakFn = fn(last: Option<char>, first: Option<char>) -> bool;

/// Strategies that decide whether a soft line break is rendered as a newline.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum LineBreakStrategy {
    /// Ignores soft line breaks if both neighbors are East Asian wide characters.
    SimpleEastAsian,
    /// Follows CSS Text Module Level 3 with some enhancements for CJK.
    CssText3,
    /// A user defined strategy.
    Custom(SoftLineBreakFn),
}

impl LineBreakStrategy {
    /// Returns true if a soft line break between `last` and `first` should be
    /// rendered as a newline.
    pub fn soft_line_break(&self, last: Option<char>, first: Option<char>) -> bool {
        match self {
            LineBreakStrategy::SimpleEastAsian => {
                !last.is_some_and(util::is_east_asian_wide_char)
                    || !first.is_some_and(util::is_east_asian_wide_char)
            }
            LineBreakStrategy::CssText3 => css_text3_soft_line_break(last, first),
            LineBreakStrategy::Custom(f) => f(last, first),
        }
    }
}

// See https://www.w3.org/TR/2020/WD-css-text-3-20200429/#line-break-transform and
// https://github.com/w3c/csswg-drafts/issues/5086
fn css_text3_soft_line_break(last: Option<char>, first: Option<char>) -> bool {
    // Rule1: a zero-width space next to the break removes the break.
    if last == Some('\u{200B}') || first == Some('\u{200B}') {
        return false;
    }

    // Rule2: if both neighbors are F, W or H and neither is Hangul, the break is removed.
    let is_fwh = |c: Option<char>| {
        c.is_some_and(|c| util::east_asian_width(c) != util::EastAsianWidth::Other)
    };
    if is_fwh(last) && is_fwh(first) {
        return last.is_some_and(util::is_hangul_char) || first.is_some_and(util::is_hangul_char);
    }

    // Rule3: a space-discarding character or U+3000 next to the break removes the break.
    let discards = |c: Option<char>| {
        c.is_some_and(|c| c == '\u{3000}' || util::is_space_discarding_unicode_char(c))
    };
    if discards(last) || discards(first) {
        return false;
    }

    // Rule4: the break is converted to a space.
    true
}

// }}} LineBreakStrategy

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
    _phantom: core::marker::PhantomData<*const W>,
}

impl<W: TextWrite> BuiltinNodesRenderer<W> {
    /// Create a new [`BuiltinNodesRenderer`] with the given options.
    pub fn new(format_options: Options) -> Self {
        Self {
            format_options: format_options.clone(),
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
            let render_p = !is_in_tight_list(arena, node_ref);
            context.push_pblock(render_p);
            if render_p {
                w.write_str("<p")?;
                write_attributes!(arena, node_ref, source, w, self.format_options, paragraph);
                w.write_str(">")?;
            }
            if let Some(task) = context.pop_task() {
                match task {
                    Task::Completed => {
                        w.write_str(r#"<input checked="" disabled="" type="checkbox""#)?;
                    }
                    Task::Active => {
                        w.write_str(r#"<input disabled="" type="checkbox""#)?;
                    }
                }
                if self.format_options.xhtml {
                    w.write_str(" /> ")?;
                } else {
                    w.write_str("> ")?;
                }
            }
        } else {
            let render_p = context.pop_pblock().unwrap_or(false);
            if render_p {
                w.write_str("</p>\n")?;
            } else {
                let n = &arena[node_ref];
                if n.next_sibling().is_some() && n.first_child().is_some() {
                    w.write_char('\n')?;
                }
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
                w.write_str("<h")?;
                write_bytes(w, &[b'0' + heading.level()])?;
                write_attributes!(arena, node_ref, source, w, self.format_options, heading);
                w.write_str(">")?;
            } else {
                w.write_str("</h")?;
                write_bytes(w, &[b'0' + heading.level()])?;
                w.write_str(">\n")?;
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
            w.write_str("<hr")?;
            write_attributes!(
                arena,
                node_ref,
                source,
                w,
                self.format_options,
                thematic_break
            );
            if self.format_options.xhtml {
                w.write_str(" />\n")?;
            } else {
                w.write_str(">\n")?;
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
            w.write_str("<pre><code")?;
            let kd = as_kind_data!(arena, node_ref, CodeBlock);
            let lang = kd.language(source);
            if let Some(lang) = lang.as_option() {
                w.write_str(" class=\"language-")?;
                lang.write_to(&mut TextWriter::new(w), source)?;
                w.write_str("\"")?;
            }
            w.write_str(">")?;
            for line in kd.value().iter(source) {
                write_text(w, &line)?;
            }
        } else {
            w.write_str("</code></pre>\n")?;
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
            w.write_str("<blockquote")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, blockquote);
            w.write_str(">\n")?;
        } else {
            w.write_str("</blockquote>\n")?;
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
            w.write_str("<")?;
            w.write_str(tag)?;
            if node.is_ordered() && node.start() != 1 {
                w.write_str(" start=\"")?;
                let mut buf = [0u8; 10];
                let mut value = node.start();
                let mut i = buf.len();
                loop {
                    i -= 1;
                    buf[i] = b'0' + (value % 10) as u8;
                    value /= 10;
                    if value == 0 {
                        break;
                    }
                }
                write_bytes(w, &buf[i..])?;
                w.write_str("\"")?;
            }
            write_attributes!(arena, node_ref, source, w, self.format_options, list);
            w.write_str(">\n")?;
        } else {
            w.write_str("</")?;
            w.write_str(tag)?;
            w.write_str(">\n")?;
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
            w.write_str("<li")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, list_item);
            w.write_str(">")?;
            if let Some(p) = arena[node_ref].parent() {
                if let KindData::List(list) = arena[p].kind_data() {
                    if let Some(first_child) = arena[node_ref].first_child() {
                        if !list.is_tight() || !matches_kind!(arena, first_child, Paragraph) {
                            w.write_char('\n')?;
                        }
                    }
                }
            }
            context.set_task(as_kind_data!(arena, node_ref, ListItem).task());
        } else {
            w.write_str("</li>\n")?;
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
                let kd = as_kind_data!(arena, node_ref, HtmlBlock);
                for line in kd.value().iter(source) {
                    write_html(w, &line)?;
                }
            } else {
                w.write_str("<!-- raw HTML omitted -->\n")?;
            }
        }
        Ok(WalkStatus::Continue)
    }

    fn render_link_reference_definition<'a>(
        &self,
        _writer: &mut W,
        _source: &'a str,
        _arena: &'a Arena,
        _node_ref: NodeRef,
        _entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        Ok(WalkStatus::SkipChildren)
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
            w.write_str("<table")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, table);
            w.write_str(">\n")?;
        } else {
            w.write_str("</table>\n")?;
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
            w.write_str("<thead")?;
            write_attributes!(
                arena,
                node_ref,
                source,
                w,
                self.format_options,
                table_header
            );
            w.write_str(">\n")?;
        } else {
            w.write_str("</thead>\n")?;
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
            w.write_str("<tbody>\n")?;
        } else {
            w.write_str("</tbody>\n")?;
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
            w.write_str("<tr")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, table_row);
            w.write_str(">\n")?;
        } else {
            w.write_str("</tr>\n")?;
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
            w.write_str("<")?;
            write_bytes(w, tag.as_bytes())?;
            if self.format_options.xhtml {
                match as_kind_data!(arena, node_ref, TableCell).alignment() {
                    TableCellAlignment::None => {}
                    n => {
                        w.write_str(r#" align=""#)?;
                        w.write_str(n.as_str())?;
                        w.write_str(r#"""#)?;
                    }
                }
            } else {
                match as_kind_data!(arena, node_ref, TableCell).alignment() {
                    TableCellAlignment::None => {}
                    n => {
                        w.write_str(r#" style="text-align: "#)?;
                        w.write_str(n.as_str())?;
                        w.write_str(r#";""#)?;
                    }
                }
            }
            write_attributes!(arena, node_ref, source, w, self.format_options, table_cell);
            w.write_str(">")?;
        } else {
            w.write_str("</")?;
            write_bytes(w, tag.as_bytes())?;
            w.write_str(">\n")?;
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
            kd.value().write_to(&mut TextWriter::new(w), source)?;
            if kd.has_qualifiers(TextQualifier::HARD_LINE_BREAK)
                || (kd.has_qualifiers(TextQualifier::SOFT_LINE_BREAK)
                    && self.format_options.hard_wraps)
            {
                if self.format_options.xhtml {
                    w.write_str("<br />\n")?;
                } else {
                    w.write_str("<br>\n")?;
                }
            } else if kd.has_qualifiers(TextQualifier::SOFT_LINE_BREAK) {
                match &self.format_options.line_break_strategy {
                    None => w.write_char('\n')?,
                    Some(strategy) => {
                        let sv = kd.value().value(source);
                        let trimmed =
                            sv.trim_end_matches(|c: char| c.is_ascii() && util::is_space(c as u8));
                        let mut last = trimmed.chars().next_back();
                        if last.is_none() {
                            last = last_char_of_siblings(arena, node_ref, source);
                        }
                        let first =
                            first_char_of_siblings(arena, arena[node_ref].next_sibling(), source);
                        if strategy.soft_line_break(last, first) {
                            w.write_char('\n')?;
                        }
                    }
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
            let kd = as_kind_data!(arena, node_ref, CodeSpan);
            w.write_str("<code")?;
            write_attributes!(arena, node_ref, source, w, self.format_options, code_span);
            w.write_str(">")?;
            kd.value().write_to(&mut TextWriter::new(w), source)?;
            return Ok(WalkStatus::SkipChildren);
        } else {
            w.write_str("</code>")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_emphasis<'a>(
        &self,
        w: &mut W,
        _source: &'a str,
        _arena: &'a Arena,
        _node_ref: NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            w.write_str("<em>")?;
        } else {
            w.write_str("</em>")?;
        }
        Ok(WalkStatus::Continue)
    }

    fn render_strong<'a>(
        &self,
        w: &mut W,
        _source: &'a str,
        _arena: &'a Arena,
        _node_ref: NodeRef,
        entering: bool,
        _context: &mut Context,
    ) -> Result<WalkStatus> {
        if entering {
            w.write_str("<strong>")?;
        } else {
            w.write_str("</strong>")?;
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
        let kd = as_kind_data!(arena, node_ref, Link);
        if entering {
            let dest = kd.destination().value(source);
            w.write_str("<a href=\"")?;
            if self.format_options.allows_unsafe || !is_dangerous_url(dest.as_bytes()) {
                write_link_url(w, &dest)?;
            }
            w.write_str("\"")?;
            if let Some(title) = kd.title().as_option() {
                w.write_str(" title=\"")?;
                title.write_to(&mut TextWriter::new(w), source)?;
                w.write_str("\"")?;
            }
            write_attributes!(arena, node_ref, source, w, self.format_options, link);
            w.write_str(">")?;
            if matches!(kd.link_kind(), LinkKind::Auto(_)) {
                if let Some(fc) = arena[node_ref].first_child() {
                    if matches_kind!(arena, fc, Text) {
                        let t = as_kind_data!(arena, fc, Text);
                        t.value().write_to(&mut TextWriter::new(w), source)?;
                        w.write_str("</a>")?;
                    }
                }
                return Ok(WalkStatus::SkipChildren);
            }
        } else {
            if !matches!(kd.link_kind(), LinkKind::Auto(_)) {
                w.write_str("</a>")?;
            }
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
            let dest = kd.destination().value(source);
            w.write_str("<img src=\"")?;
            if self.format_options.allows_unsafe || !is_dangerous_url(dest.as_bytes()) {
                write_link_url(w, &dest)?;
            }
            w.write_str("\" alt=\"")?;
            self.render_texts(w, source, arena, node_ref, ctx)?;
            w.write_str("\"")?;
            if let Some(title) = kd.title().as_option() {
                w.write_str(" title=\"")?;
                title.write_to(&mut TextWriter::new(w), source)?;
                w.write_str("\"")?;
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
                write_html(w, &kd.str(source))?;
            } else {
                w.write_str("<!-- raw HTML omitted -->")?;
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
            w.write_str("<del>")?;
        } else {
            w.write_str("</del>")?;
        }
        Ok(WalkStatus::Continue)
    }
}

// }}} BuiltinNodesRenderer

fn last_char_of_siblings(arena: &ast::Arena, node_ref: ast::NodeRef, source: &str) -> Option<char> {
    let mut cur = Some(node_ref);
    while let Some(c) = cur {
        if arena[c].kind_data().typ() != ast::NodeType::Inline {
            break;
        }
        if let Some(ch) = arena[c].last_char(arena, source) {
            return Some(ch);
        }
        cur = arena[c].previous_sibling();
    }
    None
}

fn first_char_of_siblings(
    arena: &ast::Arena,
    start: Option<ast::NodeRef>,
    source: &str,
) -> Option<char> {
    let mut cur = start;
    while let Some(c) = cur {
        if arena[c].kind_data().typ() != ast::NodeType::Inline {
            break;
        }
        if let Some(ch) = arena[c].first_char(arena, source) {
            return Some(ch);
        }
        cur = arena[c].next_sibling();
    }
    None
}

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

/// Creates an empty renderer extension.
pub const fn empty_renderer_extension() -> EmptyRendererExtension {
    EmptyRendererExtension {}
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

/// Creates a renderer extension from a closure.
pub const fn renderer_extension<'r, W, T>(f: T) -> RendererExtensionFn<T>
where
    W: TextWrite + 'r,
    T: FnOnce(&mut Renderer<'r, W>),
{
    RendererExtensionFn { f }
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
        if let Some(initializer) = &helper.format_options.context_key_registry_initializer {
            initializer.initialize(&mut helper.context_key_registry.borrow_mut());
        }
        let mut s = Self { helper };
        ext.apply(&mut s);
        s.init();
        s
    }

    fn init(&mut self) {
        self.helper.init();
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
        options: RenderOptions<'_>,
    ) -> Result<()> {
        self.helper.render(writer, source, arena, node_ref, options)
    }
}

// }}} Renderer

// Writers {{{

const REPLACEMENT_CHAR: char = '\u{FFFD}';

/// A [`TextWrite`] adapter that writes HTML content.
/// This writer replaces insecure HTML characters(null characters).
pub struct HtmlWriter<'w, W: TextWrite + ?Sized>(&'w mut W);

impl<'w, W: TextWrite + ?Sized> HtmlWriter<'w, W> {
    /// Creates a new [`HtmlWriter`] that writes to `w`.
    #[inline(always)]
    pub fn new(w: &'w mut W) -> Self {
        Self(w)
    }
}

impl<W: TextWrite + ?Sized> TextWrite for HtmlWriter<'_, W> {
    fn write_str(&mut self, s: &str) -> Result<()> {
        let bytes = s.as_bytes();
        let mut i = 0;
        while let Some(rel) = memchr::memchr(0, &bytes[i..]) {
            let j = i + rel;
            if j != i {
                self.0.write_str(&s[i..j])?;
            }
            self.0.write_char(REPLACEMENT_CHAR)?;
            i = j + 1;
        }
        if i != bytes.len() {
            self.0.write_str(&s[i..])?;
        }
        Ok(())
    }
}

/// A [`TextWrite`] adapter that writes text content.
/// This writer replaces insecure HTML characters(null characters) and
/// escapes HTML special characters(`<`, `>`, `&`, `"`).
///
/// Passing this writer to [`text::Value::write_to`](crate::text::Value::write_to)
/// decodes and encodes the value in a single pass.
pub struct TextWriter<'w, W: TextWrite + ?Sized>(&'w mut W);

/// Bytes that need HTML escaping. Keep this in sync with
/// [`try_escape_html_byte`].
const HTML_ESCAPED_BYTES: [u8; 5] = [0x00, b'"', b'&', b'<', b'>'];

/// Below this length, scanning byte by byte is cheaper than setting up the
/// jump-table search.
const TEXT_WRITER_JUMP_THRESHOLD: usize = 32;

impl<'w, W: TextWrite + ?Sized> TextWriter<'w, W> {
    /// Creates a new [`TextWriter`] that writes to `w`.
    #[inline(always)]
    pub fn new(w: &'w mut W) -> Self {
        Self(w)
    }

    fn write_short(&mut self, s: &str) -> Result<()> {
        let bytes = s.as_bytes();
        let mut n = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if let Some(rep) = try_escape_html_byte(b) {
                if i != n {
                    self.0.write_str(&s[n..i])?;
                }
                self.0.write_str(rep)?;
                n = i + 1;
            }
        }
        if n != bytes.len() {
            self.0.write_str(&s[n..])?;
        }
        Ok(())
    }
}

impl<W: TextWrite + ?Sized> TextWrite for TextWriter<'_, W> {
    fn write_str(&mut self, s: &str) -> Result<()> {
        let bytes = s.as_bytes();
        if bytes.len() < TEXT_WRITER_JUMP_THRESHOLD {
            return self.write_short(s);
        }

        // Find the first occurrence of each escapable byte, then jump between
        // them instead of examining every byte. Positions are refreshed only
        // for the bytes we have just consumed, so this stays linear even for
        // a long run of escapes.
        let mut positions = [None; 5];
        for (i, &c) in HTML_ESCAPED_BYTES.iter().enumerate() {
            positions[i] = memchr(c, bytes);
        }
        if positions.iter().all(|p| p.is_none()) {
            return self.0.write_str(s);
        }

        let mut n = 0;
        loop {
            let mut next: Option<usize> = None;
            for p in positions.into_iter().flatten() {
                if next.is_none_or(|m| p < m) {
                    next = Some(p);
                }
            }
            let Some(i) = next else {
                break;
            };
            if i > n {
                self.0.write_str(&s[n..i])?;
            }
            if let Some(rep) = try_escape_html_byte(bytes[i]) {
                self.0.write_str(rep)?;
            }
            n = i + 1;
            for (k, pos) in positions.iter_mut().enumerate() {
                if pos.is_some_and(|p| p < n) {
                    *pos = memchr(HTML_ESCAPED_BYTES[k], &bytes[n..]).map(|p| n + p);
                }
            }
        }
        if n < bytes.len() {
            self.0.write_str(&s[n..])?;
        }
        Ok(())
    }
}

/// Writes the given `s` to `w` with:
///
/// - replacing insecure HTML characters(null characters)
/// - replacing HTML entities(`<`, `>`, `&`, `"`)
///
/// `s` is not decoded. To write a [`text::Value`](crate::text::Value),
/// use [`text::Value::write_to`](crate::text::Value::write_to) with a [`TextWriter`].
#[inline]
pub fn write_text<W: TextWrite + ?Sized>(w: &mut W, s: &str) -> Result<()> {
    TextWriter(w).write_str(s)
}

/// A [`TextWrite`] adapter that writes link URL content.
/// This writer URL-escapes the content and then escapes HTML special characters.
pub struct LinkUrlWriter<'w, W: TextWrite + ?Sized>(&'w mut W);

impl<'w, W: TextWrite + ?Sized> LinkUrlWriter<'w, W> {
    /// Creates a new [`LinkUrlWriter`] that writes to `w`.
    #[inline(always)]
    pub fn new(w: &'w mut W) -> Self {
        Self(w)
    }
}

impl<W: TextWrite + ?Sized> TextWrite for LinkUrlWriter<'_, W> {
    fn write_str(&mut self, s: &str) -> Result<()> {
        let url = escape_url(s.as_bytes(), &EscapeUrlOptions::for_url());
        write_bytes(self.0, &escape_html(url))
    }
}

/// Writes the given link URL `s` to `w` with URL escaping and HTML escaping.
#[inline]
pub fn write_link_url<W: TextWrite + ?Sized>(w: &mut W, s: &str) -> Result<()> {
    LinkUrlWriter(w).write_str(s)
}

/// Writes the given `s` to `w` with replacing insecure HTML characters(null characters).
#[inline]
pub fn write_html<W: TextWrite + ?Sized>(w: &mut W, s: &str) -> Result<()> {
    HtmlWriter(w).write_str(s)
}

// }}} Writers

// Utilities {{{

/// Renders attributes to the given writer.
/// You can specify a set of valid attribute names to filter the attributes.
#[inline]
pub fn render_attributes<W: TextWrite>(
    w: &mut W,
    source: &str,
    attributes: &Attributes,
    valid: Option<&AsciiWordSet>,
) -> Result<()> {
    for (key, value) in attributes.iter() {
        if !key.starts_with("data-") && !key.starts_with("aria-") {
            if let Some(valid_set) = valid {
                if !valid_set.contains(key) {
                    continue;
                }
            }
        }
        w.write_str(" ")?;
        w.write_str(key)?;
        w.write_str("=\"")?;
        value.write_to(&mut TextWriter::new(w), source)?;
        w.write_str("\"")?;
    }
    Ok(())
}

/// Returns true if the given node is a child of a tight list, otherwise false.
#[inline(always)]
pub fn is_in_tight_list(arena: &ast::Arena, node_ref: ast::NodeRef) -> bool {
    if let Some(p) = arena[node_ref].parent() {
        if let Some(gp) = arena[p].parent() {
            if let KindData::List(list) = arena[gp].kind_data() {
                return list.is_tight();
            }
        }
    }
    false
}

#[inline]
fn write_bytes<W: TextWrite + ?Sized>(w: &mut W, bytes: &[u8]) -> Result<()> {
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

/// Context for the paragraph renderer, containing the format options, and paragraph
pub struct ParagraphRendererContext {
    format_options: Options,
    options: ParagraphRendererOptions,
}

impl ParagraphRendererContext {
    fn new(format_options: Options, options: ParagraphRendererOptions) -> Self {
        Self {
            format_options,
            options,
        }
    }

    /// Returns a reference to the format options.
    #[inline(always)]
    pub fn format_options(&self) -> &Options {
        &self.format_options
    }

    /// Returns a reference to the paragraph renderer options.
    #[inline(always)]
    pub fn options(&self) -> &ParagraphRendererOptions {
        &self.options
    }
}

/// Options for the paragraph renderer.
#[derive(Default)]
pub struct ParagraphRendererOptions {
    /// A renderer function for task list item checkboxes.
    /// NodeRef provided is the parent of the paragraph node (the task list item).
    #[allow(clippy::type_complexity)]
    pub render_task_list_item: Option<
        Box<
            dyn Fn(
                &mut dyn TextWrite,
                &str,
                &ast::Arena,
                ast::NodeRef,
                &mut Context,
                &ParagraphRendererContext,
            ) -> Result<()>,
        >,
    >,

    /// A function that determines whether a paragraph should be wrapped in `<p>` tags.
    /// If paragraph is in a tight block, it will not be wrapped in `<p>` tags.
    pub is_in_tight_block: Option<fn(&ast::Arena, ast::NodeRef) -> bool>,
}

impl RendererOptions for ParagraphRendererOptions {}

/// A renderer for paragraph nodes.
pub struct ParagraphRenderer<W: TextWrite = String> {
    _phantom: core::marker::PhantomData<*const W>,
    ctx: ParagraphRendererContext,
    is_in_tight_block: fn(&ast::Arena, ast::NodeRef) -> bool,
}

impl<W: TextWrite> ParagraphRenderer<W> {
    /// Creates a new `ParagraphRenderer` with the given HTML options and paragraph renderer
    /// options.
    pub fn with_options(html_opts: Options, options: ParagraphRendererOptions) -> Self {
        let is_in_tight_block = options.is_in_tight_block.unwrap_or(is_in_tight_list);
        Self {
            _phantom: core::marker::PhantomData,
            ctx: ParagraphRendererContext::new(html_opts, options),
            is_in_tight_block,
        }
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
            let render_p = !(self.is_in_tight_block)(arena, node_ref);
            context.push_pblock(render_p);
            if render_p {
                w.write_str("<p")?;
                write_attributes!(
                    arena,
                    node_ref,
                    source,
                    w,
                    self.ctx.format_options,
                    paragraph
                );
                w.write_str(">")?;
            }
            if let Some(task) = context.pop_task() {
                if let Some(ref r) = self.ctx.options.render_task_list_item {
                    r(
                        w,
                        source,
                        arena,
                        arena[node_ref].parent().unwrap(),
                        context,
                        &self.ctx,
                    )?;
                } else {
                    match task {
                        Task::Completed => {
                            w.write_str(r#"<input checked="" disabled="" type="checkbox""#)?;
                        }
                        Task::Active => {
                            w.write_str(r#"<input disabled="" type="checkbox""#)?;
                        }
                    }
                    if self.ctx.format_options.xhtml {
                        w.write_str(" /> ")?;
                    } else {
                        w.write_str("> ")?;
                    }
                }
            }
        } else {
            let render_p = context.pop_pblock().unwrap_or(false);
            if render_p {
                w.write_str("</p>\n")?;
            } else {
                let n = &arena[node_ref];
                if n.next_sibling().is_some() && n.first_child().is_some() {
                    w.write_char('\n')?;
                }
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
    options: impl Into<ParagraphRendererOptions>,
) -> impl RendererExtension<'r, W>
where
    W: TextWrite + 'r,
{
    renderer_extension(move |r| {
        r.add_node_renderer(ParagraphRenderer::with_options, options.into());
    })
}

// }}} ParagraphRenderer
