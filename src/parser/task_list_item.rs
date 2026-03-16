use crate::ast::{Arena, NodeRef, Task};
use crate::parser::{Context, ParagraphTransformer};
use crate::scanner::scan_task_list_item;
use crate::text::{self, Reader};
use crate::{as_kind_data_mut, as_type_data_mut, matches_kind};

/// [`ParagraphTransformer`] that transforms list items into task list items.
#[derive(Debug, Default)]
pub struct TaskListItemParagraphTransformer {}

impl TaskListItemParagraphTransformer {
    /// Returns a new [`TaskListItemParagraphTransformer`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl ParagraphTransformer for TaskListItemParagraphTransformer {
    fn transform(
        &self,
        arena: &mut Arena,
        paragraph_ref: NodeRef,
        reader: &mut text::BasicReader,
        _context: &mut Context,
    ) {
        // Given AST structure must be like
        // - List
        //   - ListItem
        //     - Paragraph
        //       (current line)
        let Some(list_item) = arena[paragraph_ref]
            .parent()
            .filter(|&gp| matches_kind!(arena, gp, ListItem))
        else {
            return;
        };
        let mut lines = as_type_data_mut!(arena, paragraph_ref, Block).take_source();
        if lines.is_empty() {
            as_type_data_mut!(arena, paragraph_ref, Block).put_back_source(lines);
            return;
        }
        let line = lines[0].bytes(reader.source());
        let Some(pos) = scan_task_list_item(&line) else {
            as_type_data_mut!(arena, paragraph_ref, Block).put_back_source(lines);
            return;
        };

        as_kind_data_mut!(arena, list_item, ListItem).set_task(
            if line[pos - 2] == b'x' || line[pos - 2] == b'X' {
                Some(Task::Checked)
            } else {
                Some(Task::Unchecked)
            },
        );
        lines[0] = lines[0].with_start(lines[0].start() + pos);
        as_type_data_mut!(arena, paragraph_ref, Block).put_back_source(lines);
    }
}
