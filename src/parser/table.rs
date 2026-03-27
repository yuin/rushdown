use core::cell::RefCell;

use alloc::{boxed::Box, rc::Rc, vec::Vec};

use crate::{
    as_kind_data, as_type_data_mut,
    ast::{
        self, Arena, CodeSpan, KindData, NodeRef, Table, TableBody, TableCell, TableCellAlignment,
        TableHeader, TableRow, WalkStatus,
    },
    context::{ContextKey, ContextKeyRegistry, ObjectValue},
    parser::{self, AstTransformer, Context, ParagraphTransformer},
    scanner::{
        scan_table_delim_center, scan_table_delim_left, scan_table_delim_none,
        scan_table_delim_right,
    },
    text::{self, Reader, Segment},
    util::{indent_width, is_blank, is_punct, is_space, trim_right_space, TinyVec},
    Result,
};

struct EscapedPipeCell {
    cell: NodeRef,
    pos: Vec<usize>,
}

fn get_escaped_pipe_cells(
    ctx: &mut Context,
    key: ContextKey<ObjectValue>,
) -> &mut Vec<EscapedPipeCell> {
    ctx.get_mut(key)
        .unwrap()
        .downcast_mut::<Vec<EscapedPipeCell>>()
        .unwrap()
}

const ESCAPED_PIPE_CELL: &str = "_epc";

/// [`ParagraphTransformer`] that transforms table paragraphs into table nodes.
#[derive(Debug)]
pub struct TableParagraphTransformer {
    escaped_pipe_cell: ContextKey<ObjectValue>,
}

impl TableParagraphTransformer {
    /// Returns a new [`TableParagraphTransformer`].
    pub fn new(reg: Rc<RefCell<ContextKeyRegistry>>) -> Self {
        let escaped_pipe_cell = reg
            .borrow_mut()
            .get_or_create::<ObjectValue>(ESCAPED_PIPE_CELL);

        Self { escaped_pipe_cell }
    }

    fn parse_row(
        &self,
        arena: &mut Arena,
        segment: &Segment,
        alignments: &[TableCellAlignment],
        is_header: bool,
        reader: &text::BasicReader,
        ctx: &mut Context,
    ) -> Option<NodeRef> {
        let source = reader.source();
        let segment = segment.trim_left_space(source).trim_right_space(source);
        let node_pos = segment.start();
        let line = segment.bytes(source);
        let mut pos = if line.first().is_some_and(|&b| b == b'|') {
            1
        } else {
            0
        };
        let limit = if line.last().is_some_and(|&b| b == b'|') {
            line.len() - 1
        } else {
            line.len()
        };

        let row_ref = arena.new_node(TableRow::new());
        arena[row_ref].set_pos(node_pos);
        let mut i = 0;
        while pos < limit {
            let alignment = if i >= alignments.len() {
                if !is_header {
                    return Some(row_ref);
                }
                TableCellAlignment::None
            } else {
                alignments[i]
            };
            let start = pos;
            let mut end = 0;
            let mut escaped_pipe_cell: Option<EscapedPipeCell> = None;
            let cell_ref = arena.new_node(TableCell::with_alignment(alignment));
            while pos < limit {
                if line[pos] == b'\\' && line.get(pos + 1).is_some_and(|&b| is_punct(b)) {
                    if line[pos + 1] == b'|' {
                        if escaped_pipe_cell.is_none() {
                            escaped_pipe_cell = Some(EscapedPipeCell {
                                cell: cell_ref,
                                pos: Vec::new(),
                            });
                        }
                        escaped_pipe_cell
                            .as_mut()
                            .unwrap()
                            .pos
                            .push(pos + segment.start());
                    }
                    pos += 2;
                } else if line[pos] == b'|' {
                    end = 1;
                    pos += 1;
                    break;
                } else {
                    pos += 1;
                }
            }
            if let Some(escaped_pipe_cell) = escaped_pipe_cell {
                if ctx.get(self.escaped_pipe_cell).is_none() {
                    let lst = Vec::<EscapedPipeCell>::new();
                    ctx.insert(self.escaped_pipe_cell, Box::new(lst));
                }
                let lst = get_escaped_pipe_cells(ctx, self.escaped_pipe_cell);
                lst.push(escaped_pipe_cell);
            }
            let mut col_seg: Segment =
                (segment.start() + start, segment.start() + pos - end).into();
            col_seg = col_seg.trim_left_space(source).trim_right_space(source);
            as_type_data_mut!(arena, cell_ref, Block).append_source_line(col_seg);
            arena[cell_ref].set_pos((segment.start() + start).saturating_sub(1));
            row_ref.append_child_fast(arena, cell_ref);
            i += 1;
        }
        while i < alignments.len() {
            let cell_ref = arena.new_node(TableCell::with_alignment(TableCellAlignment::None));
            row_ref.append_child_fast(arena, cell_ref);
            i += 1;
        }
        Some(row_ref)
    }
}

impl ParagraphTransformer for TableParagraphTransformer {
    fn transform(
        &self,
        arena: &mut Arena,
        paragraph_ref: NodeRef,
        reader: &mut text::BasicReader,
        ctx: &mut Context,
    ) {
        let mut i = 1;
        let mut start = i;
        let mut lines = as_type_data_mut!(arena, paragraph_ref, Block).take_source();
        if lines.len() < 2 {
            as_type_data_mut!(arena, paragraph_ref, Block).put_back_source(lines);
            return;
        }
        let mut alignments_opt: Option<Vec<TableCellAlignment>> = None;
        let mut header_row_ref_opt: Option<NodeRef> = None;
        while i < lines.len() {
            match parse_delimiter(&lines[i], reader) {
                Some(a) => match self.parse_row(arena, &lines[i - 1], &a, true, reader, ctx) {
                    Some(n) => {
                        if arena[n].children(arena).count() != a.len() {
                            n.delete(arena);
                            i += 1;
                            continue;
                        }
                        alignments_opt = Some(a);
                        header_row_ref_opt = Some(n);
                        i += 1;
                        start = i - 2;
                        break;
                    }
                    None => {
                        i += 1;
                        continue;
                    }
                },
                None => {
                    i += 1;
                    continue;
                }
            }
        }

        match (alignments_opt, header_row_ref_opt) {
            (Some(alignments), Some(header_row_ref)) => {
                let header_ref = arena.new_node(TableHeader::new());
                header_ref.append_child_fast(arena, header_row_ref);
                let table_ref = arena.new_node(Table::new());
                table_ref.append_child_fast(arena, header_ref);
                if let Some(pos) = arena[header_row_ref].pos() {
                    arena[header_ref].set_pos(pos);
                    arena[table_ref].set_pos(pos);
                }
                let body_ref = arena.new_node(TableBody::new());
                while i < lines.len() {
                    if let Some(row_ref) =
                        self.parse_row(arena, &lines[i], &alignments, false, reader, ctx)
                    {
                        body_ref.append_child_fast(arena, row_ref);
                    }
                    i += 1;
                }
                if let Some(fc) = arena[body_ref].first_child() {
                    table_ref.append_child_fast(arena, body_ref);
                    if let Some(pos) = arena[fc].pos() {
                        arena[body_ref].set_pos(pos);
                    }
                }
                lines.drain(start..i);
                arena[paragraph_ref].parent().unwrap().insert_after(
                    arena,
                    paragraph_ref,
                    table_ref,
                );
                if lines.is_empty() {
                    paragraph_ref.remove(arena);
                } else {
                    as_type_data_mut!(arena, paragraph_ref, Block).put_back_source(lines);
                }
            }
            _ => {
                as_type_data_mut!(arena, paragraph_ref, Block).put_back_source(lines);
            }
        }
    }
}

fn parse_delimiter(
    segment: &Segment,
    reader: &text::BasicReader,
) -> Option<Vec<TableCellAlignment>> {
    let line = segment.bytes(reader.source());
    if !is_table_delim(&line) {
        return None;
    }
    let mut cols = line.split(|&b| b == b'|').collect::<Vec<&[u8]>>();
    if is_blank(cols[0]) {
        cols.remove(0);
    }
    if !cols.is_empty() && is_blank(cols[cols.len() - 1]) {
        cols.pop();
    }
    let mut alignments = Vec::<TableCellAlignment>::new();
    for col in cols {
        if scan_table_delim_left(col).is_some_and(|l| l == col.len()) {
            alignments.push(TableCellAlignment::Left);
        } else if scan_table_delim_right(col).is_some_and(|l| l == col.len()) {
            alignments.push(TableCellAlignment::Right);
        } else if scan_table_delim_center(col).is_some_and(|l| l == col.len()) {
            alignments.push(TableCellAlignment::Center);
        } else if scan_table_delim_none(col).is_some_and(|l| l == col.len()) {
            alignments.push(TableCellAlignment::None);
        } else {
            return None;
        }
    }
    Some(alignments)
}

fn is_table_delim(bs: &[u8]) -> bool {
    let (w, _) = indent_width(bs, 0);
    if w > 3 {
        return false;
    }
    let mut all_sep = true;
    for &b in trim_right_space(bs) {
        if b != b'-' {
            all_sep = false;
        }
        if !(is_space(b) || b == b'-' || b == b'|' || b == b':') {
            return false;
        }
    }
    !all_sep
}

/// [`AstTransformer`] that transforms escaped pipe cells in tables.
#[derive(Debug)]
pub struct TableAstTransformer {
    escaped_pipe_cell: ContextKey<ObjectValue>,
}

impl TableAstTransformer {
    /// Returns a new [`TableAstTransformer`].
    pub fn new(reg: Rc<RefCell<ContextKeyRegistry>>) -> Self {
        let escaped_pipe_cell = reg
            .borrow_mut()
            .get_or_create::<ObjectValue>(ESCAPED_PIPE_CELL);
        Self { escaped_pipe_cell }
    }
}

impl AstTransformer for TableAstTransformer {
    fn transform(
        &self,
        arena: &mut Arena,
        _doc_ref: NodeRef,
        _reader: &mut text::BasicReader,
        ctx: &mut parser::Context,
    ) {
        let Some(mut lstv) = ctx.remove(self.escaped_pipe_cell) else {
            return;
        };
        let lst = lstv.downcast_mut::<Vec<EscapedPipeCell>>().unwrap();
        let mut code_spans: Vec<(NodeRef, usize)> = Vec::new();
        for (i, epc) in lst.iter().enumerate() {
            if arena.get(epc.cell).is_some() {
                ast::walk(
                    arena,
                    epc.cell,
                    &mut |arena: &Arena, node_ref: NodeRef, entering: bool| -> Result<WalkStatus> {
                        if entering {
                            if let Some(n) = arena.get(node_ref) {
                                if let KindData::CodeSpan(_) = n.kind_data() {
                                    code_spans.push((node_ref, i));
                                }
                            }
                            return Ok(WalkStatus::Continue);
                        }
                        Ok(WalkStatus::SkipChildren)
                    },
                )
                .expect("walk failed");
            }
        }

        for (code_span, i) in code_spans {
            let mut new_indices: TinyVec<text::Index> = TinyVec::empty();
            let mut modified = false;
            if let Some(indices) = as_kind_data!(arena, code_span, CodeSpan).indices() {
                for mut index in indices.iter().copied() {
                    let mut added = false;
                    'l: loop {
                        for (j, &pos) in lst[i].pos.iter().enumerate() {
                            if index.start() <= pos && pos < index.stop() {
                                modified = true;
                                let t1: text::Index = (index.start(), pos).into();
                                let t2: text::Index = (pos + 1, index.stop()).into();
                                if j != 0 {
                                    new_indices.pop();
                                }
                                new_indices.push(t1);
                                new_indices.push(t2);
                                added = true;
                                index = (pos + 1, index.stop()).into();
                                continue 'l;
                            }
                        }
                        break;
                    }
                    if !added {
                        new_indices.push(index);
                    }
                }
            }
            if modified {
                let parent = arena[code_span].parent().unwrap();
                let new_code_span = arena.new_node(CodeSpan::from_indices(new_indices));
                if let Some(pos) = arena[code_span].pos() {
                    arena[new_code_span].set_pos(pos);
                }
                parent.replace_child(arena, code_span, new_code_span);
            }
        }
    }
}
