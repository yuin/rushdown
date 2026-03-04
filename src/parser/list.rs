extern crate alloc;

use crate::context::{BoolValue, ContextKey, ContextKeyRegistry};
#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;

use alloc::rc::Rc;

use core::cell::RefCell;
use core::ops::Range;

use crate::ast::{Arena, KindData, List, ListItem, NodeRef};
use crate::parser::{is_thematic_break, matches_setext_heading_bar, BlockParser, Context, State};
use crate::text::Reader;
use crate::util::{indent_position, indent_width, is_blank};
use crate::{as_kind_data, as_kind_data_mut, as_type_data, matches_kind, text};

const SKIP_LIST_PARSER: &str = "_slp";
const EMPTY_LIST_ITEM_WITH_BLANK_LINES: &str = "_eliwbl";

/// [`BlockParser`] for lists.
#[derive(Debug)]
pub struct ListParser {
    skip_list_parser: ContextKey<BoolValue>,
    empty_list_item_with_blank_lines: ContextKey<BoolValue>,
}

impl ListParser {
    /// Returns a new [`ListParser`].
    pub fn new(reg: Rc<RefCell<ContextKeyRegistry>>) -> Self {
        let skip_list_parser = reg
            .borrow_mut()
            .get_or_create::<BoolValue>(SKIP_LIST_PARSER);
        let empty_list_item_with_blank_lines = reg
            .borrow_mut()
            .get_or_create::<BoolValue>(EMPTY_LIST_ITEM_WITH_BLANK_LINES);
        Self {
            skip_list_parser,
            empty_list_item_with_blank_lines,
        }
    }
}

impl BlockParser for ListParser {
    fn trigger(&self) -> &[u8] {
        b"-+*0123456789"
    }

    fn open(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        if let Some(last) = ctx.last_opened_block() {
            if matches_kind!(arena[last], List) {
                return None;
            }
        }

        if matches!(ctx.remove(self.skip_list_parser), Some(true)) {
            return None;
        }

        let (line, _) = reader.peek_line_bytes()?;
        let parse_result = parse_list_item(&line)?;
        if let Some(last) = ctx.last_opened_block() {
            if matches_kind!(arena, last, Paragraph) && arena[last].parent() == Some(parent_ref) {
                // we allow only lists starting with 1 to interrupt paragraphs.
                if parse_result.typ == ListItemMarkerType::Ordered
                    && parse_result.start_number != Some(1)
                {
                    return None;
                }
                // an empty list item cannot interrupt a paragraph.
                if parse_result.is_blank_content {
                    return None;
                }
            }
        }
        let node_ref = arena.new_node(List::new(parse_result.marker_char));
        if let Some(start_number) = parse_result.start_number {
            as_kind_data_mut!(arena, node_ref, List).set_start(start_number);
        }
        ctx.insert(self.empty_list_item_with_blank_lines, false);
        Some((node_ref, State::HAS_CHILDREN))
    }

    fn cont(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<State> {
        let Some((line, _)) = reader.peek_line_bytes() else {
            return Some(State::HAS_CHILDREN);
        };
        if is_blank(&line) {
            if let Some(last_child_ref) = arena[node_ref].last_child() {
                if !arena[last_child_ref].has_children() {
                    ctx.insert(self.empty_list_item_with_blank_lines, true);
                }
            }
            return Some(State::HAS_CHILDREN);
        }

        // "offset" means a width that bar indicates.
        //    -  aaaaaaaa
        // |----|
        //
        // If the indent is less than the last offset like
        // - a
        //  - b          <--- current line
        // it maybe a new child of the list.
        //
        // Empty list items can have multiple blanklines
        //
        // -             <--- 1st item is an empty thus "offset" is unknown
        //
        //
        //   -           <--- current line
        //
        // -> 1 list with 2 blank items
        //
        // So if the last item is an empty, it maybe a new child of the list.

        let offset = last_offset(arena, node_ref);
        let last_is_empty = arena[node_ref]
            .last_child()
            .is_some_and(|r| !arena[r].has_children());
        let (indent, _) = indent_width(&line, reader.line_offset());

        if indent < offset || last_is_empty {
            if indent < 4 {
                if let Some(parse_result) = parse_list_item(&line) {
                    if parse_result.marker.start.saturating_sub(offset) < 4 {
                        let lst = as_kind_data!(arena, node_ref, List);
                        if !lst.can_continue(parse_result.marker_char, parse_result.is_ordered()) {
                            return None;
                        }
                        // Thematic Breaks take precedence over lists
                        if is_thematic_break(&line[parse_result.marker.start..], 0) {
                            let mut is_heading = false;
                            if let Some(last) = ctx.last_opened_block() {
                                if matches_kind!(arena, last, Paragraph) {
                                    if let Some(c) =
                                        matches_setext_heading_bar(&line[parse_result.marker.end..])
                                    {
                                        is_heading = c == b'-';
                                    }
                                }
                            }
                            if !is_heading {
                                return None;
                            }
                        }
                        return Some(State::HAS_CHILDREN);
                    }
                }
            }
            if !last_is_empty {
                return None;
            }
        }

        if last_is_empty && indent < offset {
            return None;
        }

        // Non empty items can not exist next to an empty list item
        // with blank lines. So we need to close the current list
        //
        // -
        //
        //   foo
        //
        // -> 1 list with 1 blank items and 1 paragraph
        if matches!(ctx.get(self.empty_list_item_with_blank_lines), Some(true)) {
            return None;
        }

        Some(State::HAS_CHILDREN)
    }

    fn close(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        _reader: &mut text::BasicReader,
        _ctx: &mut Context,
    ) {
        let mut c = arena[node_ref].first_child();
        let mut is_tight = true;
        while let Some(child_ref) = c {
            let gc = arena[child_ref].first_child();
            if let Some(grand_child_ref) = gc {
                if gc != arena[child_ref].last_child() {
                    let mut c1 = arena[grand_child_ref].next_sibling();
                    while let Some(child1_ref) = c1 {
                        if as_type_data!(arena, child1_ref, Block).has_blank_previous_line() {
                            is_tight = false;
                            break;
                        }
                        c1 = arena[child1_ref].next_sibling();
                    }
                }
            }
            if c != arena[node_ref].first_child()
                && as_type_data!(arena, child_ref, Block).has_blank_previous_line()
            {
                is_tight = false;
                break;
            }
            c = arena[child_ref].next_sibling();
        }
        as_kind_data_mut!(arena, node_ref, List).set_tight(is_tight);
    }

    fn can_interrupt_paragraph(&self) -> bool {
        true
    }
}

/// [`BlockParser`] for list items.
#[derive(Debug)]
pub struct ListItemParser {
    skip_list_parser: ContextKey<BoolValue>,
    empty_list_item_with_blank_lines: ContextKey<BoolValue>,
}

impl ListItemParser {
    /// Returns a new [`ListItemParser`].
    pub fn new(reg: Rc<RefCell<ContextKeyRegistry>>) -> Self {
        let skip_list_parser = reg
            .borrow_mut()
            .get_or_create::<BoolValue>(SKIP_LIST_PARSER);
        let empty_list_item_with_blank_lines = reg
            .borrow_mut()
            .get_or_create::<BoolValue>(EMPTY_LIST_ITEM_WITH_BLANK_LINES);
        Self {
            skip_list_parser,
            empty_list_item_with_blank_lines,
        }
    }
}

impl BlockParser for ListItemParser {
    fn trigger(&self) -> &[u8] {
        b"-+*0123456789"
    }

    fn open(
        &self,
        arena: &mut Arena,
        parent_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<(NodeRef, State)> {
        // In some cases, it can be parsed as a list item, but invalid as a list.
        // e.g.
        //
        // - empty list items can not interrupt paragraphs(can not start new lists)
        if !matches_kind!(arena[parent_ref], List) {
            return None;
        }

        let offset = last_offset(arena, parent_ref);
        let (line, _) = reader.peek_line_bytes()?;
        let parse_result = parse_list_item(&line)?;
        if parse_result.marker.start.saturating_sub(offset) > 3 {
            return None;
        }
        ctx.insert(self.empty_list_item_with_blank_lines, false);
        let item_offset = parse_result.offset;
        let node_ref = arena.new_node(ListItem::with_offset(parse_result.marker.end + item_offset));
        if parse_result.is_blank_content {
            return Some((node_ref, State::NO_CHILDREN));
        }
        let content = parse_result.content?;
        let (pos, padding) = indent_position(&line[content.start..], content.start, item_offset)?;
        let child = parse_result.marker.end + pos;
        reader.advance_and_set_padding(child, padding);
        Some((node_ref, State::HAS_CHILDREN))
    }

    fn cont(
        &self,
        arena: &mut Arena,
        node_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) -> Option<State> {
        let Some((line, _)) = reader.peek_line_bytes() else {
            return Some(State::HAS_CHILDREN);
        };

        if is_blank(&line) {
            reader.advance_to_eol();
            return Some(State::HAS_CHILDREN);
        }
        let offset = last_offset(arena, arena[node_ref].parent().unwrap());
        let empty_item_with_blank_lines =
            matches!(ctx.get(self.empty_list_item_with_blank_lines), Some(true));
        let is_empty = !arena[node_ref].has_children() && empty_item_with_blank_lines;
        let (indent, _) = indent_width(&line, reader.line_offset());
        if (is_empty || indent < offset) && indent < 4 {
            // new list item found
            if parse_list_item(&line).is_some() {
                ctx.insert(self.skip_list_parser, true);
                return None;
            }
            if !is_empty {
                return None;
            }
        }
        let (pos, padding) = indent_position(&line, reader.line_offset(), offset)?;
        reader.advance_and_set_padding(pos, padding);

        Some(State::HAS_CHILDREN)
    }

    fn can_interrupt_paragraph(&self) -> bool {
        true
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ListItemMarkerType {
    Unordered,
    Ordered,
}

struct ListItemParseResult {
    typ: ListItemMarkerType,
    marker: Range<usize>,
    marker_char: u8,
    content: Option<Range<usize>>,
    is_blank_content: bool,
    offset: usize,
    start_number: Option<u32>,
}

impl ListItemParseResult {
    fn is_ordered(&self) -> bool {
        self.typ == ListItemMarkerType::Ordered
    }
}

fn last_offset(arena: &Arena, node_ref: NodeRef) -> usize {
    if let Some(last_child_ref) = arena[node_ref].last_child() {
        if let KindData::ListItem(item) = arena[last_child_ref].kind_data() {
            return item.offset();
        }
    }
    0
}

fn parse_list_item(line: &[u8]) -> Option<ListItemParseResult> {
    let mut i = 0;
    while i < line.len() {
        let c = line[i];
        if c == b' ' {
            i += 1;
            continue;
        }
        if c == b'\t' {
            return None;
        }
        break;
    }
    if i > 3 {
        return None;
    }

    let marker_start = i;
    let marker_end: usize;
    let typ: ListItemMarkerType;
    if i < line.len() && line[i] == b'-' || line[i] == b'*' || line[i] == b'+' {
        i += 1;
        marker_end = i;
        typ = ListItemMarkerType::Unordered;
    } else if i < line.len() {
        while i < line.len() && line[i].is_ascii_digit() {
            i += 1;
        }
        if i == marker_start || i - marker_start > 9 {
            return None;
        }
        if i < line.len() && (line[i] == b'.' || line[i] == b')') {
            i += 1;
            marker_end = i;
            typ = ListItemMarkerType::Ordered;
        } else {
            return None;
        }
    } else {
        return None;
    }
    if i < line.len() && line[i] != b'\n' {
        let (w, _) = indent_width(&line[i..], 0);
        if w == 0 {
            return None;
        }
    }
    let start_number = if typ == ListItemMarkerType::Ordered {
        let num_str = str::from_utf8(&line[marker_start..marker_end - 1]).ok()?;
        Some(num_str.parse::<u32>().ok()?)
    } else {
        None
    };
    if i >= line.len() {
        return Some(ListItemParseResult {
            typ,
            marker: marker_start..marker_end,
            marker_char: line[marker_end - 1],
            content: None,
            is_blank_content: true,
            offset: 1,
            start_number,
        });
    }
    let content_start = i;
    let mut content_end = line.len();
    if line[content_end - 1] == b'\n' && line[i] != b'\n' {
        content_end -= 1;
    }
    let is_blank_content = is_blank(&line[content_start..content_end]);
    Some(ListItemParseResult {
        typ,
        marker: marker_start..marker_end,
        marker_char: line[marker_end - 1],
        content: Some(content_start..content_end),
        is_blank_content,
        offset: if is_blank_content {
            1
        } else {
            let (offset, _) = indent_width(&line[content_start..content_end], content_start);
            if offset > 4 {
                // offseted codeblock
                1
            } else {
                offset
            }
        },
        start_number,
    })
}
