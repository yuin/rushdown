//! AST (Abstract Syntax Tree) module for the document representation.
//!
//! This module defines the core structures and functionalities for representing
//! and manipulating the AST of a document. It includes definitions for nodes,
//! node types, attributes, and various node operations.
//!
//! # Related macros
//!
//! rushdown provides several helper macros for working with AST nodes:
//!
//! - [`crate::matches_kind!`] - Helper macro to match kind data.
//! - [`crate::as_type_data!`] - Helper macro to downcast type data.
//! - [`crate::as_type_data_mut!`] - Helper macro to downcast mutable type data.
//! - [`crate::as_kind_data!`] - Helper macro to downcast kind data.
//! - [`crate::as_kind_data_mut!`] - Helper macro to downcast mutable kind data.
//! - [`crate::matches_extension_kind!`] - Helper macro to match extension kind.
//! - [`crate::as_extension_data!`] - Helper macro to downcast extension data.
//! - [`crate::as_extension_data_mut!`] - Helper macro to downcast mutable extension data.
//!
//! # Basic usage
//! ```rust
//!
//! use rushdown::ast::*;
//! use rushdown::{as_type_data_mut, as_type_data, as_kind_data};
//! use rushdown::text::Segment;
//!
//! let mut arena = Arena::new();
//! let source = "Hello, World!";
//! let doc_ref = arena.new_node(Document::new());
//! let paragraph_ref = arena.new_node(Paragraph::new());
//! let seg = Segment::new(0, source.len());
//! as_type_data_mut!(&mut arena[paragraph_ref], Block).append_line(seg);
//! let text_ref = arena.new_node(Text::new(seg));
//! paragraph_ref.append_child(&mut arena, text_ref);
//! doc_ref.append_child(&mut arena, paragraph_ref);
//!
//! assert_eq!(arena[paragraph_ref].first_child().unwrap(), text_ref);
//! assert_eq!(
//!     as_kind_data!(&arena[text_ref], Text).str(source),
//!     "Hello, World!"
//! );
//! assert_eq!(
//!     as_type_data!(&arena[paragraph_ref], Block)
//!         .lines()
//!         .first()
//!         .unwrap()
//!         .str(source),
//!     "Hello, World!"
//! );
//! ```
//!
//! Nodes are stored in an arena for efficient memory management and access.
//! Each node is identified by a [`NodeRef`], which contains the index and unique ID of the node.
//!
//! You can get and manipulate nodes using the [`Arena`] and its methods.
//!
//! ```should_panic
//! use rushdown::ast::*;
//!
//! let mut arena = Arena::new();
//! let source = "Hello, World!";
//! let doc_ref = arena.new_node(Document::new());
//! let paragraph_ref = arena.new_node(Paragraph::new());
//! paragraph_ref.delete(&mut arena);
//!
//! let p = &arena[paragraph_ref]; // panics
//! ```
//!
//! ```rust
//! use rushdown::ast::*;
//!
//! let mut arena = Arena::new();
//! let source = "Hello, World!";
//! let doc_ref = arena.new_node(Document::new());
//! let paragraph_ref = arena.new_node(Paragraph::default());
//!
//! assert!(arena.get(paragraph_ref).is_some());
//!
//! paragraph_ref.delete(&mut arena);
//!
//! assert!(arena.get(paragraph_ref).is_none());
//! ```
//!
//! Each node belongs to a specific type and kind.
//!
//! ```text
//! - Node
//!    - type_data: node type(block or inline) specific data
//!    - kind_data: node kind(e.g. Text, Paragraph) specific data
//!    - parent, first_child, next_sibling... : relationships
//! ```
//!
//!

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::any::Any;
use core::error::Error as CoreError;
use core::fmt::{self, Debug, Display, Formatter, Write};
use core::iter::{FromIterator, IntoIterator};
use core::ops::{Index, IndexMut};
use core::result::Result as CoreResult;

use crate::error::*;
use crate::text;
use crate::util::HashMap;

use bitflags::bitflags;

// NodeRef {{{

/// Represents a referene to a node in the document.
#[derive(Default, Debug, Clone, Copy)]
pub struct NodeRef {
    cell: usize,
    id: usize,
}

impl NodeRef {
    /// Creates a new NodeRef with the given index and id.
    pub const fn new(cell: usize, id: usize) -> Self {
        Self { cell, id }
    }
}

impl PartialEq for NodeRef {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for NodeRef {}

/// An undefined NodeRef constant.
pub const NODE_REF_UNDEFINED: NodeRef = NodeRef {
    cell: usize::MAX,
    id: usize::MAX,
};

impl Display for NodeRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "NodeRef(cell:{}, id:{})", self.cell, self.id)
    }
}

impl NodeRef {
    /// Appends a child node to this node.
    /// This is a fast version that does not check for existing parent.
    pub(crate) fn append_child_fast(self, arena: &mut Arena, child_ref: NodeRef) {
        let lastref_opt = arena[self].last_child;
        {
            let s = &mut arena[self];
            s.last_child = Some(child_ref);
            if lastref_opt.is_none() {
                s.first_child = Some(child_ref);
            }
        }
        {
            let child = &mut arena[child_ref];
            child.parent = Some(self);
            child.previous_sibling = lastref_opt;
        }
        if let Some(lastref) = lastref_opt {
            arena[lastref].next_sibling = Some(child_ref);
        } else {
            arena[self].first_child = Some(child_ref);
        }
    }

    /// Appends a child node to this node.
    ///
    /// # Panics
    /// Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn append_child(self, arena: &mut Arena, child_ref: NodeRef) {
        self.try_append_child(arena, child_ref).unwrap();
    }

    /// Appends a child node to this node.
    pub fn try_append_child(self, arena: &mut Arena, child_ref: NodeRef) -> Result<()> {
        if arena[child_ref].parent().is_some() {
            child_ref.try_remove(arena)?;
        }
        let lastref_opt = arena.get_result(self)?.last_child;
        arena[self].last_child = Some(child_ref);
        if lastref_opt.is_none() {
            arena[self].first_child = Some(child_ref);
        }
        {
            let child = arena.get_mut_result(child_ref)?;
            child.parent = Some(self);
            child.previous_sibling = lastref_opt;
        }
        if let Some(lastref) = lastref_opt {
            arena.get_mut_result(lastref)?.next_sibling = Some(child_ref);
        } else {
            arena[self].first_child = Some(child_ref);
        }
        Ok(())
    }

    /// Replace a child node with another node.
    /// Replaced child will be hard removed.
    ///
    /// # Panics
    /// Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn replace_child(self, arena: &mut Arena, target_ref: NodeRef, replacer_ref: NodeRef) {
        self.try_replace_child(arena, target_ref, replacer_ref)
            .unwrap();
    }

    /// Replace a child node with another node.
    /// Replaced child will be hard removed.
    pub fn try_replace_child(
        self,
        arena: &mut Arena,
        target_ref: NodeRef,
        replacer_ref: NodeRef,
    ) -> Result<()> {
        if arena.get_result(target_ref)?.parent != Some(self) {
            return Err(Error::invalid_node_operation(format!(
                "Target node {:?} is not a child of node {:?}",
                target_ref, self
            )));
        }
        let previous_ref_opt = arena[target_ref].previous_sibling;
        let next_ref_opt = arena[target_ref].next_sibling;

        {
            let replacer = arena.get_mut_result(replacer_ref)?;
            replacer.parent = Some(self);
            replacer.previous_sibling = previous_ref_opt;
            replacer.next_sibling = next_ref_opt;
        }

        if let Some(prev_ref) = previous_ref_opt {
            arena.get_mut_result(prev_ref)?.next_sibling = Some(replacer_ref);
        } else {
            arena.get_mut_result(self)?.first_child = Some(replacer_ref);
        }

        if let Some(next_ref) = next_ref_opt {
            arena.get_mut_result(next_ref)?.previous_sibling = Some(replacer_ref);
        } else {
            arena.get_mut_result(self)?.last_child = Some(replacer_ref);
        }

        // Clear the target node in the arena
        arena.arena[target_ref.cell] = None;
        arena.free_indicies.push(target_ref.cell);
        Ok(())
    }

    /// Inserts a child node before a target node.
    ///
    /// # Panics
    /// Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn insert_before(self, arena: &mut Arena, target_ref: NodeRef, insertee_ref: NodeRef) {
        self.try_insert_before(arena, target_ref, insertee_ref)
            .unwrap();
    }

    /// Inserts a child node before a target node.
    pub fn try_insert_before(
        self,
        arena: &mut Arena,
        target_ref: NodeRef,
        insertee_ref: NodeRef,
    ) -> Result<()> {
        if arena[insertee_ref].parent().is_some() {
            insertee_ref.try_remove(arena)?;
        }
        if arena.get_result(target_ref)?.parent != Some(self) {
            return Err(Error::invalid_node_operation(format!(
                "Target node {:?} is not a child of node {:?}",
                target_ref, self
            )));
        }
        let prev = arena[target_ref].previous_sibling;
        {
            let insertee = arena.get_mut_result(insertee_ref)?;
            insertee.parent = Some(self);
            insertee.next_sibling = Some(target_ref);
            insertee.previous_sibling = prev;
        }

        if let Some(prev_ref) = arena[target_ref].previous_sibling {
            arena.get_mut_result(prev_ref)?.next_sibling = Some(insertee_ref);
        } else {
            arena.get_mut_result(self)?.first_child = Some(insertee_ref);
        }

        arena[target_ref].previous_sibling = Some(insertee_ref);
        if let Some(fc_ref) = arena.get_result(self)?.first_child {
            if fc_ref == target_ref {
                arena.get_mut_result(self)?.first_child = Some(insertee_ref);
            }
        }
        Ok(())
    }

    /// Inserts a child node after a target node.
    ///
    /// # Panics
    /// Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn insert_after(self, arena: &mut Arena, target_ref: NodeRef, insertee_ref: NodeRef) {
        self.try_insert_after(arena, target_ref, insertee_ref)
            .unwrap();
    }

    /// Inserts a child node after a target node.
    pub fn try_insert_after(
        self,
        arena: &mut Arena,
        target_ref: NodeRef,
        insertee_ref: NodeRef,
    ) -> Result<()> {
        if arena[insertee_ref].parent().is_some() {
            insertee_ref.try_remove(arena)?;
        }
        if arena.get_result(target_ref)?.parent != Some(self) {
            return Err(Error::invalid_node_operation(format!(
                "Target node {:?} is not a child of node {:?}",
                target_ref, self
            )));
        }
        let next = arena[target_ref].next_sibling;
        {
            let insertee = arena.get_mut_result(insertee_ref)?;
            insertee.parent = Some(self);
            insertee.previous_sibling = Some(target_ref);
            insertee.next_sibling = next;
        }

        if let Some(next_ref) = arena[target_ref].next_sibling {
            arena.get_mut_result(next_ref)?.previous_sibling = Some(insertee_ref);
        } else {
            arena.get_mut_result(self)?.last_child = Some(insertee_ref);
        }

        arena[target_ref].next_sibling = Some(insertee_ref);
        if let Some(lc_ref) = arena.get_result(self)?.last_child {
            if lc_ref == target_ref {
                arena.get_mut_result(self)?.last_child = Some(insertee_ref);
            }
        }
        Ok(())
    }

    /// Removes this node from the document.
    /// This does not remove its children.
    /// This does not remove the node from Arena.
    ///
    /// # Panics
    /// Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn remove(self, arena: &mut Arena) {
        self.try_remove(arena).unwrap();
    }

    /// Removes this node from the document.
    /// This does not remove its children.
    /// This does not remove the node from Arena.
    pub fn try_remove(self, arena: &mut Arena) -> Result<()> {
        let parent_ref_opt;
        let previous_ref_opt;
        let next_ref_opt;
        {
            let node = arena.get_result(self)?;
            parent_ref_opt = node.parent;
            previous_ref_opt = node.previous_sibling;
            next_ref_opt = node.next_sibling;
        }
        if let Some(parent_ref) = parent_ref_opt {
            let mparent = arena.get_mut_result(parent_ref)?;
            if mparent.first_child == Some(self) {
                mparent.first_child = next_ref_opt;
            }
            if mparent.last_child == Some(self) {
                mparent.last_child = previous_ref_opt;
            }
        }
        if let Some(previous_ref) = previous_ref_opt {
            let mprevious = arena.get_mut_result(previous_ref)?;
            mprevious.next_sibling = next_ref_opt;
        }
        if let Some(next_ref) = next_ref_opt {
            let mnext = arena.get_mut_result(next_ref)?;
            mnext.previous_sibling = previous_ref_opt;
        }
        let node = arena.get_mut_result(self)?;
        node.parent = None;
        node.previous_sibling = None;
        node.next_sibling = None;
        Ok(())
    }

    /// Deletes this node from the document, including all its children.
    /// This removes the node from Arena.
    ///
    /// # Panics
    ///
    /// Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn delete(self, arena: &mut Arena) {
        self.try_delete(arena).unwrap();
    }

    /// Removes this node from the document, including all its children.
    /// This removes the node from Arena.
    pub fn try_delete(self, arena: &mut Arena) -> Result<()> {
        let parent_ref_opt;
        let previous_ref_opt;
        let next_ref_opt;
        let first_ref_opt;
        {
            let node = arena.get_result(self)?;
            parent_ref_opt = node.parent;
            previous_ref_opt = node.previous_sibling;
            next_ref_opt = node.next_sibling;
            first_ref_opt = node.first_child;
        }
        if let Some(parent_ref) = parent_ref_opt {
            let mparent = arena.get_mut_result(parent_ref)?;
            if mparent.first_child == Some(self) {
                mparent.first_child = next_ref_opt;
            }
            if mparent.last_child == Some(self) {
                mparent.last_child = previous_ref_opt;
            }
        }
        if let Some(previous_ref) = previous_ref_opt {
            let mprevious = arena.get_mut_result(previous_ref)?;
            mprevious.next_sibling = next_ref_opt;
        }
        if let Some(next_ref) = next_ref_opt {
            let mnext = arena.get_mut_result(next_ref)?;
            mnext.previous_sibling = previous_ref_opt;
        }
        if first_ref_opt.is_some() {
            let mut currentref_opt = first_ref_opt;
            while let Some(current_ref) = currentref_opt {
                let current = arena.get_mut_result(current_ref)?;
                let nextref_opt = current.next_sibling;
                current_ref.try_delete(arena)?;
                currentref_opt = nextref_opt;
            }
        }
        // Clear the node in the arena
        arena.arena[self.cell] = None;
        arena.free_indicies.push(self.cell);
        Ok(())
    }

    // Merges a given segment into the last child of this node if
    // it can be merged, otherwise creates a new Text node and appends it to after current
    // last child.
    //
    // # Panics
    // Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn merge_or_append_text_segment(self, arena: &mut Arena, segment: text::Segment) {
        self.try_merge_or_append_text_segment(arena, segment)
            .unwrap();
    }

    // Merges a given segment into the last child of this node if
    // it can be merged, otherwise creates a new Text node and appends it to after current
    // last child.
    pub fn try_merge_or_append_text_segment(
        self,
        arena: &mut Arena,
        segment: text::Segment,
    ) -> Result<()> {
        if let Some(last_child_ref) = arena.get_result(self)?.last_child {
            if let KindData::Text(text_node) = arena.get_mut_result(last_child_ref)?.kind_data_mut()
            {
                if let Some(s) = text_node.segment() {
                    if s.stop() == segment.start()
                        && !text_node.has_qualifiers(TextQualifier::SOFT_LINE_BREAK)
                        && !text_node.has_qualifiers(TextQualifier::TEMP)
                    {
                        text_node.textual = s.with_stop(segment.stop()).into();
                        return Ok(());
                    }
                }
            }
        }
        let new_node_ref = arena.new_node(Text::new(segment));
        self.try_append_child(arena, new_node_ref)?;
        Ok(())
    }

    /// Merges a given segment into the text node after the target node if
    /// it can be merged, otherwise creates a new Text node and inserts it after the target node.
    ///
    // # Panics
    /// Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn merge_or_insert_after_text_segment(
        self,
        arena: &mut Arena,
        target_ref: NodeRef,
        segment: text::Segment,
    ) {
        self.try_merge_or_insert_after_text_segment(arena, target_ref, segment)
            .unwrap();
    }

    /// Merges a given segment into the text node after the target node if
    /// it can be merged, otherwise creates a new Text node and inserts it after the target node.
    pub fn try_merge_or_insert_after_text_segment(
        self,
        arena: &mut Arena,
        target_ref: NodeRef,
        segment: text::Segment,
    ) -> Result<()> {
        if let KindData::Text(text_node) = arena.get_mut_result(target_ref)?.kind_data_mut() {
            if let Some(s) = text_node.segment() {
                if s.stop() == segment.start()
                    && !text_node.has_qualifiers(TextQualifier::SOFT_LINE_BREAK)
                    && !text_node.has_qualifiers(TextQualifier::TEMP)
                {
                    text_node.textual = Textual::Segment(s.with_stop(segment.stop()));
                    return Ok(());
                }
            }
        }
        let new_node_ref = arena.new_node(Text::new(segment));
        self.try_insert_after(arena, target_ref, new_node_ref)?;
        Ok(())
    }

    /// Merges a given segment into the text node before the target node if
    /// it can be merged, otherwise creates a new Text node and inserts it before the target node.
    ///
    /// # Panics
    /// Panics if the operation fails(e.g. due to invalid NodeRef).
    #[inline(always)]
    pub fn merge_or_insert_before_text_segment(
        self,
        arena: &mut Arena,
        target_ref: NodeRef,
        segment: text::Segment,
    ) {
        self.try_merge_or_insert_before_text_segment(arena, target_ref, segment)
            .unwrap();
    }

    /// Merges a given segment into the text node before the target node if
    /// it can be merged, otherwise creates a new Text node and inserts it before the target node.
    pub fn try_merge_or_insert_before_text_segment(
        self,
        arena: &mut Arena,
        target_ref: NodeRef,
        segment: text::Segment,
    ) -> Result<()> {
        if let KindData::Text(text_node) = arena.get_mut_result(target_ref)?.kind_data_mut() {
            if let Some(s) = text_node.segment() {
                if s.start() == segment.stop() && !text_node.has_qualifiers(TextQualifier::TEMP) {
                    text_node.textual = Textual::Segment(s.with_start(segment.start()));
                    return Ok(());
                }
            }
        }
        let new_node_ref = arena.new_node(Text::new(segment));
        self.try_insert_before(arena, target_ref, new_node_ref)?;
        Ok(())
    }
}

// }}}

// Arena {{{

/// Options for creating a new arena.
#[derive(Debug, Clone, Copy)]
pub struct ArenaOptions {
    /// Size of the initial arena for nodes.
    /// This defaults to 1024.
    pub initial_size: usize,
}

impl Default for ArenaOptions {
    fn default() -> Self {
        Self { initial_size: 1024 }
    }
}

/// Represents an arena for storing nodes in the single document.
#[derive(Debug)]
pub struct Arena {
    /// The arena of nodes.
    arena: Vec<Option<Node>>,

    /// Unused (freed) indices in the arena.
    free_indicies: Vec<usize>,

    id_seq: usize,

    doc: Option<NodeRef>,
}

impl Default for Arena {
    fn default() -> Self {
        Self::with_options(ArenaOptions::default())
    }
}

impl Arena {
    /// Creates a new arena with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new arena with the specified options.
    pub fn with_options(options: ArenaOptions) -> Self {
        let mut s = Self {
            arena: Vec::with_capacity(options.initial_size),
            free_indicies: Vec::new(),
            id_seq: 0,
            doc: None,
        };
        s.doc = Some(s.new_node(Document::default()));
        s
    }

    /// Returns a Node for the given NodeRef.
    #[inline(always)]
    pub fn get(&self, id: NodeRef) -> Option<&Node> {
        self.arena.get(id.cell).and_then(|node| node.as_ref())
    }

    #[inline(always)]
    fn get_result(&self, id: NodeRef) -> Result<&Node> {
        self.arena
            .get(id.cell)
            .and_then(|node| node.as_ref())
            .ok_or_else(|| Error::invalid_node_ref(id))
    }

    /// Returns a mutable node to a Node for the given NodeRef.
    #[inline(always)]
    pub fn get_mut(&mut self, id: NodeRef) -> Option<&mut Node> {
        self.arena.get_mut(id.cell).and_then(|node| node.as_mut())
    }

    #[inline(always)]
    fn get_mut_result(&mut self, id: NodeRef) -> Result<&mut Node> {
        self.arena
            .get_mut(id.cell)
            .and_then(|node| node.as_mut())
            .ok_or_else(|| Error::invalid_node_ref(id))
    }

    /// Returns the document node.
    #[inline(always)]
    pub fn document(&self) -> NodeRef {
        self.doc.unwrap()
    }

    /// Creates a new node ref with the given data.
    /// `kind` must consistent with the type of `data`.
    pub fn new_node<T: Into<KindData> + 'static>(&mut self, data: T) -> NodeRef {
        let cell = if let Some(index) = self.free_indicies.pop() {
            index
        } else {
            self.arena.len()
        };

        if cell >= self.arena.len() {
            self.arena.push(None);
        }

        let node = Node::new(data);
        self.arena[cell] = Some(node);
        let node_ref = NodeRef::new(cell, self.id_seq);
        self.id_seq += 1;
        node_ref
    }
}

/// Implements indexing for Arena to access nodes by NodeRef.
/// This panics if the NodeRef is invalid.
impl Index<NodeRef> for Arena {
    type Output = Node;
    fn index(&self, node_ref: NodeRef) -> &Self::Output {
        self.arena[node_ref.cell]
            .as_ref()
            .expect("Invalid node reference")
    }
}

/// Implements mutable indexing for Arena to access nodes by NodeRef.
/// This panics if the NodeRef is invalid.
impl IndexMut<NodeRef> for Arena {
    fn index_mut(&mut self, node_ref: NodeRef) -> &mut Self::Output {
        self.arena[node_ref.cell]
            .as_mut()
            .expect("Invalid node reference")
    }
}

// }}}

// TextMap(Metadata & Attributes) {{{

/// A map of text values associated with string keys.
#[derive(Default)]
pub struct TextMap {
    store: HashMap<String, text::Value>,
}

impl Debug for TextMap {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.store.iter()).finish()
    }
}

impl TextMap {
    /// Creates a new, empty TextMap.
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
        }
    }

    /// Clears all entries in the TextMap.
    pub fn clear(&mut self) {
        self.store.clear();
    }

    /// Checks if the TextMap contains the specified key.
    pub fn contains_key(&self, key: &str) -> bool {
        self.store.contains_key(key)
    }

    /// Gets a reference to the value associated with the specified key.
    pub fn get(&self, key: &str) -> Option<&text::Value> {
        self.store.get(key)
    }

    /// Gets a mutable reference to the value associated with the specified key.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<text::Value>) {
        self.store.insert(key.into(), value.into());
    }

    /// Removes the entry associated with the specified key.
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    /// Returns an iterator over the entries in the TextMap.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &text::Value)> {
        self.store.iter()
    }

    /// Returns a mutable iterator over the entries in the TextMap.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut text::Value)> {
        self.store.iter_mut()
    }

    /// Returns an iterator over the keys in the TextMap.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.store.keys()
    }

    /// Returns an iterator over the values in the TextMap.
    pub fn values(&self) -> impl Iterator<Item = &text::Value> {
        self.store.values()
    }
}

pub struct TextMapIntoIter(<HashMap<String, text::Value> as IntoIterator>::IntoIter);
pub struct TextMapIter<'a>(<&'a HashMap<String, text::Value> as IntoIterator>::IntoIter);
pub struct TextMapIterMut<'a>(<&'a mut HashMap<String, text::Value> as IntoIterator>::IntoIter);

impl IntoIterator for TextMap {
    type Item = (String, text::Value);
    type IntoIter = TextMapIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        TextMapIntoIter(self.store.into_iter())
    }
}

impl<'a> IntoIterator for &'a TextMap {
    type Item = (&'a String, &'a text::Value);
    type IntoIter = TextMapIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        TextMapIter((&self.store).into_iter())
    }
}

impl<'a> IntoIterator for &'a mut TextMap {
    type Item = (&'a String, &'a mut text::Value);
    type IntoIter = TextMapIterMut<'a>;

    fn into_iter(self) -> Self::IntoIter {
        TextMapIterMut((&mut self.store).into_iter())
    }
}

impl Iterator for TextMapIntoIter {
    type Item = (String, text::Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}
impl<'a> Iterator for TextMapIter<'a> {
    type Item = (&'a String, &'a text::Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}
impl<'a> Iterator for TextMapIterMut<'a> {
    type Item = (&'a String, &'a mut text::Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl FromIterator<(String, text::Value)> for TextMap {
    fn from_iter<T: IntoIterator<Item = (String, text::Value)>>(iter: T) -> Self {
        let mut store = HashMap::new();
        store.extend(iter);
        Self { store }
    }
}

impl Extend<(String, text::Value)> for TextMap {
    fn extend<T: IntoIterator<Item = (String, text::Value)>>(&mut self, iter: T) {
        self.store.extend(iter);
    }
}

impl<'a> Extend<(&'a String, &'a text::Value)> for TextMap
where
    text::Value: Clone,
{
    fn extend<T: IntoIterator<Item = (&'a String, &'a text::Value)>>(&mut self, iter: T) {
        self.store
            .extend(iter.into_iter().map(|(k, v)| (k.clone(), v.clone())));
    }
}

/// Metadata associated with a document.
pub type Metadata = TextMap;

/// Attributes associated with a node.
pub type Attributes = TextMap;

// }}} TextMap(Metadata & Attributes)

// Node & Data {{{

/// Represents the data of a node in the document.
/// This is an enum that can hold different types of node data.
#[derive(Debug)]
#[non_exhaustive]
pub enum KindData {
    Document(Document),
    Paragraph(Paragraph),
    Heading(Heading),
    ThematicBreak(ThematicBreak),
    CodeBlock(CodeBlock),
    Blockquote(Blockquote),
    List(List),
    ListItem(ListItem),
    HtmlBlock(HtmlBlock),
    Text(Text),
    CodeSpan(CodeSpan),
    Emphasis(Emphasis),
    Link(Link),
    Image(Image),
    RawHtml(RawHtml),

    Table(Table),
    TableHeader(TableHeader),
    TableBody(TableBody),
    TableRow(TableRow),
    TableCell(TableCell),

    Strikethrough(Strikethrough),

    Extension(Box<dyn ExtensionData>),
}

impl KindData {
    /// Returns the type of the node.
    pub fn typ(&self) -> NodeType {
        match self {
            KindData::Document(n) => n.typ(),
            KindData::Paragraph(n) => n.typ(),
            KindData::Heading(n) => n.typ(),
            KindData::ThematicBreak(n) => n.typ(),
            KindData::CodeBlock(n) => n.typ(),
            KindData::Blockquote(n) => n.typ(),
            KindData::List(n) => n.typ(),
            KindData::ListItem(n) => n.typ(),
            KindData::HtmlBlock(n) => n.typ(),
            KindData::Text(n) => n.typ(),
            KindData::CodeSpan(n) => n.typ(),
            KindData::Emphasis(n) => n.typ(),
            KindData::Link(n) => n.typ(),
            KindData::Image(n) => n.typ(),
            KindData::RawHtml(n) => n.typ(),

            KindData::Table(n) => n.typ(),
            KindData::TableHeader(n) => n.typ(),
            KindData::TableBody(n) => n.typ(),
            KindData::TableRow(n) => n.typ(),
            KindData::TableCell(n) => n.typ(),

            KindData::Strikethrough(n) => n.typ(),

            KindData::Extension(n) => n.typ(),
        }
    }

    /// Returns the kind name of the node.
    pub fn kind_name(&self) -> &'static str {
        match self {
            KindData::Document(n) => n.kind_name(),
            KindData::Paragraph(n) => n.kind_name(),
            KindData::Heading(n) => n.kind_name(),
            KindData::ThematicBreak(n) => n.kind_name(),
            KindData::CodeBlock(n) => n.kind_name(),
            KindData::Blockquote(n) => n.kind_name(),
            KindData::List(n) => n.kind_name(),
            KindData::ListItem(n) => n.kind_name(),
            KindData::HtmlBlock(n) => n.kind_name(),
            KindData::Text(n) => n.kind_name(),
            KindData::CodeSpan(n) => n.kind_name(),
            KindData::Emphasis(n) => n.kind_name(),
            KindData::Link(n) => n.kind_name(),
            KindData::Image(n) => n.kind_name(),
            KindData::RawHtml(n) => n.kind_name(),

            KindData::Table(n) => n.kind_name(),
            KindData::TableHeader(n) => n.kind_name(),
            KindData::TableBody(n) => n.kind_name(),
            KindData::TableRow(n) => n.kind_name(),
            KindData::TableCell(n) => n.kind_name(),

            KindData::Strikethrough(n) => n.kind_name(),

            KindData::Extension(n) => n.kind_name(),
        }
    }

    /// Returns true if the node is atomic (has no children).
    pub fn is_atomic(&self) -> bool {
        match self {
            KindData::Document(n) => n.is_atomic(),
            KindData::Paragraph(n) => n.is_atomic(),
            KindData::Heading(n) => n.is_atomic(),
            KindData::ThematicBreak(n) => n.is_atomic(),
            KindData::CodeBlock(n) => n.is_atomic(),
            KindData::Blockquote(n) => n.is_atomic(),
            KindData::List(n) => n.is_atomic(),
            KindData::ListItem(n) => n.is_atomic(),
            KindData::HtmlBlock(n) => n.is_atomic(),
            KindData::Text(n) => n.is_atomic(),
            KindData::CodeSpan(n) => n.is_atomic(),
            KindData::Emphasis(n) => n.is_atomic(),
            KindData::Link(n) => n.is_atomic(),
            KindData::Image(n) => n.is_atomic(),
            KindData::RawHtml(n) => n.is_atomic(),

            KindData::Table(n) => n.is_atomic(),
            KindData::TableHeader(n) => n.is_atomic(),
            KindData::TableBody(n) => n.is_atomic(),
            KindData::TableRow(n) => n.is_atomic(),
            KindData::TableCell(n) => n.is_atomic(),

            KindData::Strikethrough(n) => n.is_atomic(),

            KindData::Extension(n) => n.is_atomic(),
        }
    }

    /// Pretty prints the node data.
    pub fn pretty_print(&self, w: &mut dyn Write, source: &str, level: usize) -> fmt::Result {
        match self {
            KindData::Document(n) => n.pretty_print(w, source, level),
            KindData::Paragraph(n) => n.pretty_print(w, source, level),
            KindData::Heading(n) => n.pretty_print(w, source, level),
            KindData::ThematicBreak(n) => n.pretty_print(w, source, level),
            KindData::CodeBlock(n) => n.pretty_print(w, source, level),
            KindData::Blockquote(n) => n.pretty_print(w, source, level),
            KindData::List(n) => n.pretty_print(w, source, level),
            KindData::ListItem(n) => n.pretty_print(w, source, level),
            KindData::HtmlBlock(n) => n.pretty_print(w, source, level),
            KindData::Text(n) => n.pretty_print(w, source, level),
            KindData::CodeSpan(n) => n.pretty_print(w, source, level),
            KindData::Emphasis(n) => n.pretty_print(w, source, level),
            KindData::Link(n) => n.pretty_print(w, source, level),
            KindData::Image(n) => n.pretty_print(w, source, level),
            KindData::RawHtml(n) => n.pretty_print(w, source, level),

            KindData::Table(n) => n.pretty_print(w, source, level),
            KindData::TableHeader(n) => n.pretty_print(w, source, level),
            KindData::TableBody(n) => n.pretty_print(w, source, level),
            KindData::TableRow(n) => n.pretty_print(w, source, level),
            KindData::TableCell(n) => n.pretty_print(w, source, level),

            KindData::Strikethrough(n) => n.pretty_print(w, source, level),

            KindData::Extension(n) => n.pretty_print(w, source, level),
        }
    }
}

/// An enum representing the type of a node: Block or Inline.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[non_exhaustive]
pub enum NodeType {
    ContainerBlock,
    LeafBlock,
    Inline,
}

impl From<NodeType> for TypeData {
    fn from(t: NodeType) -> Self {
        match t {
            NodeType::ContainerBlock => TypeData::Block(Block {
                btype: BlockType::Container,
                ..Default::default()
            }),
            NodeType::LeafBlock => TypeData::Block(Block {
                btype: BlockType::Leaf,
                ..Default::default()
            }),
            NodeType::Inline => TypeData::Inline(Inline {}),
        }
    }
}

/// A Data associated with its [`NodeType`].
#[derive(Debug)]
#[non_exhaustive]
pub enum TypeData {
    Block(Block),
    Inline(Inline),
}

/// Types of blocks.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockType {
    Container,
    Leaf,
}

/// A Data associated with block type nodes.
#[derive(Debug)]
pub struct Block {
    btype: BlockType,

    lines: Option<Vec<text::Segment>>,

    has_blank_previous_line: bool,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            btype: BlockType::Container,
            lines: Some(Vec::new()),
            has_blank_previous_line: false,
        }
    }
}

impl Block {
    /// Returns true if this block is a container block.
    #[inline(always)]
    pub fn is_container(&self) -> bool {
        self.btype == BlockType::Container
    }

    /// Returns true if this block is a leaf block.
    #[inline(always)]
    pub fn is_leaf(&self) -> bool {
        self.btype == BlockType::Leaf
    }

    /// Takes the lines of this block, leaving None in its place.
    pub fn take_lines(&mut self) -> Vec<text::Segment> {
        self.lines.take().unwrap()
    }

    /// Puts back the lines of this block.
    pub fn put_back_lines(&mut self, lines: Vec<text::Segment>) {
        self.lines = Some(lines);
    }

    /// Returns the lines of this block.
    #[inline(always)]
    pub fn lines(&self) -> &text::Block {
        self.lines.as_deref().unwrap_or(&[])
    }

    /// Appends a line to this block.
    #[inline(always)]
    pub fn append_line(&mut self, line: text::Segment) {
        if let Some(lines) = &mut self.lines {
            lines.push(line);
        } else {
            self.lines = Some(vec![line]);
        }
    }

    /// Unshifts a line to this block.
    #[inline(always)]
    pub fn unshift_line(&mut self, line: text::Segment) {
        if let Some(lines) = &mut self.lines {
            lines.insert(0, line);
        } else {
            self.lines = Some(vec![line]);
        }
    }

    /// Appends lines to this block.
    #[inline(always)]
    pub fn append_lines(&mut self, lines: &text::Block) {
        if let Some(self_lines) = &mut self.lines {
            self_lines.extend_from_slice(lines);
        } else {
            self.lines = Some(lines.to_vec());
        }
    }

    /// Replaces a line at the given index with the given line.
    #[inline(always)]
    pub fn replace_line(&mut self, index: usize, line: text::Segment) {
        if let Some(lines) = &mut self.lines {
            if index < lines.len() {
                lines[index] = line;
            }
        }
    }

    /// Removes a line at the given index.
    #[inline(always)]
    pub fn remove_line(&mut self, index: usize) {
        if let Some(lines) = &mut self.lines {
            if index < lines.len() {
                lines.remove(index);
            }
        }
    }

    /// Returns true if this block has a blank line before it.
    #[inline(always)]
    pub fn has_blank_previous_line(&self) -> bool {
        self.has_blank_previous_line
    }

    /// Sets whether this block has a blank line before it.
    #[inline(always)]
    pub fn set_blank_previous_line(&mut self, value: bool) {
        self.has_blank_previous_line = value;
    }
}

/// A Data associated with inline type nodes.
#[derive(Debug)]
pub struct Inline {}

/// Represents a node in the document.
#[derive(Debug)]
pub struct Node {
    kind_data: KindData,
    type_data: TypeData,
    parent: Option<NodeRef>,
    first_child: Option<NodeRef>,
    next_sibling: Option<NodeRef>,
    previous_sibling: Option<NodeRef>,
    last_child: Option<NodeRef>,
    attributes: Attributes,
}

impl Node {
    pub fn new<T: Into<KindData> + 'static>(data: T) -> Self {
        let d: KindData = data.into();
        let t: NodeType = d.typ();
        Self {
            kind_data: d,
            type_data: t.into(),
            parent: None,
            first_child: None,
            next_sibling: None,
            previous_sibling: None,
            last_child: None,
            attributes: Attributes::default(),
        }
    }

    /// Returns the data of the node.
    pub fn kind_data(&self) -> &KindData {
        &self.kind_data
    }

    /// Returns mutable data of the node.
    pub fn kind_data_mut(&mut self) -> &mut KindData {
        &mut self.kind_data
    }

    /// Returns the data of the node for its type.
    pub fn type_data(&self) -> &TypeData {
        &self.type_data
    }

    /// Returns mutable data of the node for its type.
    pub fn type_data_mut(&mut self) -> &mut TypeData {
        &mut self.type_data
    }

    /// Returns the parent of the node.
    #[inline(always)]
    pub fn parent(&self) -> Option<NodeRef> {
        self.parent
    }

    /// Sets the parent of the node.
    #[inline(always)]
    pub fn set_parent(&mut self, parent: NodeRef) {
        self.parent = Some(parent);
    }

    /// Returns the first child of the node.
    #[inline(always)]
    pub fn first_child(&self) -> Option<NodeRef> {
        self.first_child
    }

    /// Returns true if has any child.
    #[inline(always)]
    pub fn has_children(&self) -> bool {
        self.first_child.is_some()
    }

    /// Sets the first child of the node.
    #[inline(always)]
    pub fn set_first_child(&mut self, child: NodeRef) {
        self.first_child = Some(child);
    }

    /// Returns the next sibling of the node.
    #[inline(always)]
    pub fn next_sibling(&self) -> Option<NodeRef> {
        self.next_sibling
    }

    /// Sets the next sibling of the node.
    #[inline(always)]
    pub fn set_next_sibling(&mut self, sibling: NodeRef) {
        self.next_sibling = Some(sibling);
    }

    /// Returns the previous sibling of the node.
    #[inline(always)]
    pub fn previous_sibling(&self) -> Option<NodeRef> {
        self.previous_sibling
    }

    /// Sets the previous sibling of the node.
    #[inline(always)]
    pub fn set_previous_sibling(&mut self, sibling: NodeRef) {
        self.previous_sibling = Some(sibling);
    }

    /// Returns the last child of the node.
    #[inline(always)]
    pub fn last_child(&self) -> Option<NodeRef> {
        self.last_child
    }

    /// Sets the last child of the node.
    #[inline(always)]
    pub fn set_last_child(&mut self, child: NodeRef) {
        self.last_child = Some(child);
    }

    /// Returns the last child of the node.
    /// Since this method keeps arena reference,
    /// you can not mutate the node data through this method.
    /// Mutating the node data requires a mutable reference to the arena.
    #[inline(always)]
    pub fn children<'a>(&self, arena: &'a Arena) -> Siblings<'a> {
        Siblings {
            arena,
            front: self.first_child,
            back: self.last_child,
        }
    }

    /// Returns the children of the node as a vector.
    /// Since this method does keep arena reference,
    /// you can mutate the node data through this method.
    #[inline(always)]
    pub fn children_mut(&self, arena: &Arena) -> NodesMut {
        NodesMut::with_vec(self.children(arena).collect())
    }

    /// Returns the attributes of the node.
    #[inline(always)]
    pub fn attributes(&self) -> &Attributes {
        &self.attributes
    }

    /// Returns mutable attributes of the node.
    #[inline(always)]
    pub fn attributes_mut(&mut self) -> &mut Attributes {
        &mut self.attributes
    }
}

/// An iterator over the siblings of a node in the arena.
pub struct Siblings<'a> {
    arena: &'a Arena,
    front: Option<NodeRef>,
    back: Option<NodeRef>,
}

impl Iterator for Siblings<'_> {
    type Item = NodeRef;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.front {
            let node = self.arena.get(current)?;
            self.front = node.next_sibling;
            if self.back.is_none() {
                self.back = Some(current);
            }
            Some(current)
        } else {
            None
        }
    }
}

impl DoubleEndedIterator for Siblings<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.back {
            let node = self.arena.get(current)?;
            self.back = node.previous_sibling;
            if self.front.is_none() {
                self.front = Some(current);
            }
            Some(current)
        } else {
            None
        }
    }
}

/// A mutable iterator over nodes in the arena.
pub struct NodesMut {
    iter: vec::IntoIter<NodeRef>,
}

impl NodesMut {
    /// Creates a new NodesMut from a vector of NodeRefs.
    fn with_vec(vec: Vec<NodeRef>) -> Self {
        NodesMut {
            iter: vec.into_iter(),
        }
    }
}

impl Iterator for NodesMut {
    type Item = NodeRef;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl DoubleEndedIterator for NodesMut {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}

/// Trait for pretty printing nodes.
pub trait PrettyPrint {
    /// Pretty prints the node data.
    fn pretty_print(&self, w: &mut dyn Write, source: &str, level: usize) -> fmt::Result;
}

/// Trait for node kinds.
pub trait NodeKind {
    /// Returns the type of this kind belongs to.
    fn typ(&self) -> NodeType;

    /// Returns the kind name.
    fn kind_name(&self) -> &'static str;

    /// Returns true if this node has no children.
    fn is_atomic(&self) -> bool {
        false
    }
}

fn pp(
    w: &mut dyn Write,
    arena: &Arena,
    node_ref: NodeRef,
    source: &str,
    level: usize,
) -> fmt::Result {
    let indent = pp_indent(level);
    {
        let node = arena.get(node_ref).ok_or(fmt::Error)?;

        writeln!(w, "{}{} {{", indent, node.kind_data.kind_name())?;
        node.kind_data.pretty_print(w, source, level + 1)?;
    }
    let indent2 = pp_indent(level + 1);
    let indent3 = pp_indent(level + 2);
    writeln!(w, "{}Ref: {}", indent2, node_ref)?;
    if let TypeData::Block(block) = arena[node_ref].type_data() {
        if block.lines().is_empty() {
            writeln!(w, "{}Lines: []", indent2)?;
        } else {
            writeln!(w, "{}Lines: [", indent2)?;
            for line in block.lines() {
                write!(w, "{}{}", indent3, line.str(source))?;
            }
            writeln!(w)?;
            writeln!(w, "{}]", indent2)?;
        }
        if block.has_blank_previous_line() {
            writeln!(w, "{}HasBlankPreviousLine: true", indent2)?;
        } else {
            writeln!(w, "{}HasBlankPreviousLine: false", indent2)?;
        }
    }

    if !arena[node_ref].attributes().is_empty() {
        write!(w, "{}Attributes ", indent2)?;
        writeln!(w, "{{")?;
        for (key, value) in arena[node_ref].attributes() {
            write!(w, "{}{}: ", indent3, key)?;
            writeln!(w, "{}", value.str(source))?;
        }
        write!(w, "{}", indent2)?;
        writeln!(w, "}}")?;
    }

    for child in arena[node_ref].children(arena) {
        pp(w, arena, child, source, level + 1)?;
    }

    write!(w, "{}", indent)?;
    writeln!(w, "}}")
}

/// Pretty prints the AST.
pub fn pretty_print(
    w: &mut dyn Write,
    arena: &Arena,
    node_ref: NodeRef,
    source: &str,
) -> fmt::Result {
    pp(w, arena, node_ref, source, 0)
}

/// Returns a string with indentation for pretty printing.
pub fn pp_indent(level: usize) -> String {
    "  ".repeat(level)
}

// }}}

// Walk {{{

/// Status for walking the AST.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WalkStatus {
    /// Indicates no more walking needed.
    Stop,

    /// Indicates that Walk wont walk on children of current node.
    SkipChildren,

    /// Indicates that Walk should continue walking.
    Continue,

    /// Indicates that Walk is done.
    Ok,
}

/// Trait for walking the AST.
/// You can not mutate the AST while walking it.
/// If you want to mutate the AST, collect the node refs and mutate them after walking.
pub trait Walk<E> {
    fn walk(
        &mut self,
        arena: &Arena,
        node_ref: NodeRef,
        entering: bool,
    ) -> CoreResult<WalkStatus, E>;
}

impl<F, E> Walk<E> for F
where
    F: FnMut(&Arena, NodeRef, bool) -> CoreResult<WalkStatus, E>,
{
    fn walk(
        &mut self,
        arena: &Arena,
        node_ref: NodeRef,
        entering: bool,
    ) -> CoreResult<WalkStatus, E> {
        self(arena, node_ref, entering)
    }
}

/// Walks the AST starting from the given node reference.
/// You can not mutate the AST while walking it.
/// If you want to mutate the AST, collect the node refs and mutate them after walking.
///
/// # Examples
///
/// ```rust
/// use core::result::Result;
/// use core::error::Error;
/// use core::fmt::{self, Display, Formatter};
/// use rushdown::ast::*;
/// use rushdown::matches_kind;
///
/// #[derive(Debug)]
/// enum UserError { SomeError(&'static str) }
///
/// impl Error for UserError {}
///
/// impl Display for UserError {
///     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
///         match self { UserError::SomeError(msg) => write!(f, "UserError: {}", msg) }
///     }
/// }
///
/// let mut arena = Arena::default();
/// let doc_ref = arena.new_node(Document::new());
/// let paragraph_ref1 = arena.new_node(Paragraph::default());
/// let text1 = arena.new_node(Text::new("Hello, World!"));
/// let paragraph_ref2 = arena.new_node(Paragraph::default());
/// let text2 = arena.new_node(Text::new("This is a test."));
///
/// doc_ref.append_child(&mut arena, paragraph_ref1);
/// paragraph_ref1.append_child(&mut arena, text1);
/// doc_ref.append_child(&mut arena, paragraph_ref2);
/// paragraph_ref2.append_child(&mut arena, text2);
///
/// let mut target: Option<NodeRef> = None;
///
/// walk(&arena, doc_ref, &mut |arena: &Arena,
///                             node_ref: NodeRef,
///                             entering: bool| -> Result<WalkStatus, UserError > {
///     if entering {
///         if let Some(fc) = arena[node_ref].first_child() {
///             if let KindData::Text(t) = &arena[fc].kind_data() {
///                 if t.str("").contains("test") {
///                     target = Some(node_ref);
///                 }
///                 if t.str("").contains("error") {
///                     return Err(UserError::SomeError("Some error occurred"));
///                 }
///             }
///         }
///     }
///     Ok(WalkStatus::Continue)
/// }).ok();
/// assert_eq!(target, Some(paragraph_ref2));
/// ```
///
pub fn walk<E: CoreError + 'static>(
    arena: &Arena,
    node_ref: NodeRef,
    walker: &mut impl Walk<E>,
) -> CoreResult<WalkStatus, CallbackError<E>> {
    let status = walker
        .walk(arena, node_ref, true)
        .map_err(CallbackError::Callback)?;
    if status == WalkStatus::Stop {
        return Ok(WalkStatus::Stop);
    }

    if status != WalkStatus::SkipChildren {
        let node = arena
            .get_result(node_ref)
            .map_err(CallbackError::Internal)?;
        let mut child_opt = node.first_child();
        while let Some(child_ref) = child_opt {
            let child_node = arena
                .get_result(child_ref)
                .map_err(CallbackError::Internal)?;
            if walk(arena, child_ref, walker)? == WalkStatus::Stop {
                return Ok(WalkStatus::Stop);
            }
            child_opt = child_node.next_sibling();
        }
    }

    if walker
        .walk(arena, node_ref, false)
        .map_err(CallbackError::Callback)?
        == WalkStatus::Stop
    {
        return Ok(WalkStatus::Stop);
    }

    Ok(WalkStatus::Ok)
}

// }}} Walk

// Blocks {{{

//   Document {{{

/// Represents the root document node.
#[derive(Debug, Default)]
pub struct Document {
    meta: Metadata,
}

impl Document {
    /// Creates a new [`Document`] node.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the metadata of the document.
    #[inline(always)]
    pub fn metadata(&self) -> &Metadata {
        &self.meta
    }

    /// Returns mutable attributes of the document.
    #[inline(always)]
    pub fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.meta
    }
}

impl NodeKind for Document {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "Document"
    }
}

impl PrettyPrint for Document {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<Document> for KindData {
    fn from(data: Document) -> Self {
        KindData::Document(data)
    }
}

//   }}}

//   Paragraph {{{

/// Represents a paragraph node.
#[derive(Debug, Default)]
pub struct Paragraph {}

impl Paragraph {
    /// Creates a new [`Paragraph`] node.
    pub fn new() -> Self {
        Self::default()
    }
}

impl NodeKind for Paragraph {
    fn typ(&self) -> NodeType {
        NodeType::LeafBlock
    }

    fn kind_name(&self) -> &'static str {
        "Paragraph"
    }
}

impl PrettyPrint for Paragraph {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<Paragraph> for KindData {
    fn from(data: Paragraph) -> Self {
        KindData::Paragraph(data)
    }
}

//   }}} Paragraph

//   Heading {{{

/// Represents a heading node.
#[derive(Debug, Default)]
pub struct Heading {
    level: u8,
}

impl Heading {
    /// Creates a new [`Heading`] with the given level.
    pub fn new(level: u8) -> Self {
        Self { level }
    }

    /// Returns the level of the heading.
    #[inline(always)]
    pub fn level(&self) -> u8 {
        self.level
    }
}

impl NodeKind for Heading {
    fn typ(&self) -> NodeType {
        NodeType::LeafBlock
    }

    fn kind_name(&self) -> &'static str {
        "Heading"
    }
}

impl PrettyPrint for Heading {
    fn pretty_print(&self, w: &mut dyn Write, _source: &str, level: usize) -> fmt::Result {
        writeln!(w, "{}Level: {}", pp_indent(level), self.level())
    }
}

impl From<Heading> for KindData {
    fn from(data: Heading) -> Self {
        KindData::Heading(data)
    }
}

//   }}} Heading

//   ThematicBreak {{{

/// Represents a thematic break node.
#[derive(Debug, Default)]
pub struct ThematicBreak {}

impl ThematicBreak {
    /// Creates a new [`ThematicBreak`] node.
    pub fn new() -> Self {
        Self::default()
    }
}

impl NodeKind for ThematicBreak {
    fn typ(&self) -> NodeType {
        NodeType::LeafBlock
    }

    fn kind_name(&self) -> &'static str {
        "ThematicBreak"
    }
}

impl PrettyPrint for ThematicBreak {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<ThematicBreak> for KindData {
    fn from(data: ThematicBreak) -> Self {
        KindData::ThematicBreak(data)
    }
}

//   }}} ThematicBreak

//   CodeBlock {{{

/// Types of code blocks.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodeBlockType {
    Indented,
    Fenced,
}

#[derive(Debug)]
pub(crate) struct FenceData {
    pub char: u8,
    pub indent: usize,
    pub length: usize,
}

/// Represents a code block node.
#[derive(Debug)]
pub struct CodeBlock {
    code_block_type: CodeBlockType,
    info: Option<text::Value>,
    fdata: Option<FenceData>,
}

impl CodeBlock {
    /// Creates a new [`CodeBlock`] node.
    pub fn new(typ: CodeBlockType, info: Option<text::Value>) -> Self {
        Self {
            code_block_type: typ,
            info,
            fdata: None,
        }
    }

    pub(crate) fn fence_data(&self) -> Option<&FenceData> {
        self.fdata.as_ref()
    }

    pub(crate) fn set_fence_data(&mut self, fdata: FenceData) {
        self.fdata = Some(fdata);
    }

    /// Returns the info string of the fenced code block.
    #[inline(always)]
    pub fn info_str<'a>(&'a self, source: &'a str) -> Option<&'a str> {
        match &self.info {
            Some(info) => Some(info.str(source)),
            None => None,
        }
    }

    /// Returns the info value of the fenced code block.
    #[inline(always)]
    pub fn info(&self) -> Option<&text::Value> {
        self.info.as_ref()
    }

    /// Returns the language of the fenced code block, if specified.
    pub fn language<'a>(&'a self, source: &'a str) -> Option<&'a str> {
        match &self.info {
            Some(info) => {
                let info_str = info.str(source);
                info_str
                    .find(' ')
                    .map(|i| &info_str[..i])
                    .or(Some(info_str))
            }
            None => None,
        }
    }
}

impl NodeKind for CodeBlock {
    fn typ(&self) -> NodeType {
        NodeType::LeafBlock
    }

    fn kind_name(&self) -> &'static str {
        "CodeBlock"
    }

    fn is_atomic(&self) -> bool {
        true
    }
}

impl PrettyPrint for CodeBlock {
    fn pretty_print(&self, w: &mut dyn Write, source: &str, level: usize) -> fmt::Result {
        writeln!(
            w,
            "{}CodeBlockType: {}",
            pp_indent(level),
            match self.code_block_type {
                CodeBlockType::Indented => "Indented",
                CodeBlockType::Fenced => "Fenced",
            }
        )?;
        writeln!(
            w,
            "{}Info: {}",
            pp_indent(level),
            match self.info_str(source) {
                Some(info) => info,
                None => &"<none>",
            }
        )
    }
}

impl From<CodeBlock> for KindData {
    fn from(data: CodeBlock) -> Self {
        KindData::CodeBlock(data)
    }
}

//   }}} CodeBlock

//   Blockquote {{{

/// Represents a block quote node.
#[derive(Debug, Default)]
pub struct Blockquote {}

impl Blockquote {
    /// Creates a new [`Blockquote`] node.
    pub fn new() -> Self {
        Self::default()
    }
}

impl NodeKind for Blockquote {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "Blockquote"
    }
}

impl PrettyPrint for Blockquote {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<Blockquote> for KindData {
    fn from(data: Blockquote) -> Self {
        KindData::Blockquote(data)
    }
}

//   }}} Blockquote

//   List {{{

/// Represents a list node.
#[derive(Debug, Default)]
pub struct List {
    marker: u8,

    is_tight: bool,

    start: u32,
}

impl List {
    /// Creates a new [`List`] node.
    pub fn new(marker: u8) -> Self {
        Self {
            marker,
            is_tight: true,
            start: 0,
        }
    }

    /// Returns true if this list is an ordered list.
    #[inline(always)]
    pub fn is_ordered(&self) -> bool {
        self.marker == b'.' || self.marker == b')'
    }

    /// Returns true if this list can continue with
    /// the given mark and a list type, otherwise false.
    pub fn can_continue(&self, marker: u8, is_ordered: bool) -> bool {
        marker == self.marker && is_ordered == self.is_ordered()
    }

    /// Returns the list marker character like '-', '+', ')' and '.'..
    #[inline(always)]
    pub fn marker(&self) -> u8 {
        self.marker
    }

    // Returns a true if this list is a 'tight' list.
    // See <https://spec.commonmark.org/0.30/#loose> for details.
    #[inline(always)]
    pub fn is_tight(&self) -> bool {
        self.is_tight
    }

    /// Sets whether the list is tight.
    #[inline(always)]
    pub fn set_tight(&mut self, tight: bool) {
        self.is_tight = tight;
    }

    /// Returns  an initial number of this ordered list.
    /// If this list is not an ordered list, start is 0.
    #[inline(always)]
    pub fn start(&self) -> u32 {
        self.start
    }

    /// Sets the initial number of this ordered list.
    #[inline(always)]
    pub fn set_start(&mut self, start: u32) {
        self.start = start;
    }
}

impl NodeKind for List {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "List"
    }
}

impl PrettyPrint for List {
    fn pretty_print(&self, w: &mut dyn Write, _source: &str, level: usize) -> fmt::Result {
        writeln!(w, "{}Marker: '{}'", pp_indent(level), self.marker() as char)?;
        writeln!(w, "{}IsTight: {}", pp_indent(level), self.is_tight())?;
        writeln!(w, "{}Start: {}", pp_indent(level), self.start())
    }
}

impl From<List> for KindData {
    fn from(data: List) -> Self {
        KindData::List(data)
    }
}

//   }}} List

//   ListItem {{{

/// Task status for list items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Task {
    Unchecked,
    Checked,
}

impl Display for Task {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Task::Unchecked => write!(f, "Unchecked"),
            Task::Checked => write!(f, "Checked"),
        }
    }
}

/// Represents a list item node.
#[derive(Debug, Default)]
pub struct ListItem {
    offset: usize,
    task: Option<Task>,
}

impl ListItem {
    /// Creates a new [`ListItem`] node.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new [`ListItem`] with the given offset.
    pub(crate) fn with_offset(offset: usize) -> Self {
        Self { offset, task: None }
    }

    /// Returns an offset position of this item.
    #[inline(always)]
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    /// Returns true if this item is a task.
    #[inline(always)]
    pub fn is_task(&self) -> bool {
        self.task.is_some()
    }

    /// Returns a task of this item.
    #[inline(always)]
    pub fn task(&self) -> Option<Task> {
        self.task
    }

    /// Sets a task of this item.
    #[inline(always)]
    pub fn set_task(&mut self, task: Option<Task>) {
        self.task = task;
    }
}

impl NodeKind for ListItem {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "ListItem"
    }
}

impl PrettyPrint for ListItem {
    fn pretty_print(&self, w: &mut dyn Write, _source: &str, level: usize) -> fmt::Result {
        if self.offset() > 0 {
            writeln!(w, "{}Offset: {}", pp_indent(level), self.offset())?;
        }
        if self.is_task() {
            writeln!(w, "{}Task: {}", pp_indent(level), self.task().unwrap())
        } else {
            Ok(())
        }
    }
}

impl From<ListItem> for KindData {
    fn from(data: ListItem) -> Self {
        KindData::ListItem(data)
    }
}

//   }}} ListItem

//   HtmlBlock {{{

/// HTMLBlockType represents kinds of an html blocks.
/// See <https://spec.commonmark.org/0.30/#html-blocks>
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HtmlBlockType {
    Type1,
    Type2,
    Type3,
    Type4,
    Type5,
    Type6,
    Type7,
}

impl Display for HtmlBlockType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            HtmlBlockType::Type1 => write!(f, "Type1"),
            HtmlBlockType::Type2 => write!(f, "Type2"),
            HtmlBlockType::Type3 => write!(f, "Type3"),
            HtmlBlockType::Type4 => write!(f, "Type4"),
            HtmlBlockType::Type5 => write!(f, "Type5"),
            HtmlBlockType::Type6 => write!(f, "Type6"),
            HtmlBlockType::Type7 => write!(f, "Type7"),
        }
    }
}

/// Represents an HTML block node.
#[derive(Debug)]
pub struct HtmlBlock {
    typ: HtmlBlockType,
}

impl HtmlBlock {
    /// Creates a new [`HtmlBlock`] with the given type.
    pub fn new(typ: HtmlBlockType) -> Self {
        Self { typ }
    }

    // Returns an html block type of this item.
    #[inline(always)]
    pub fn block_type(&self) -> HtmlBlockType {
        self.typ
    }
}

impl NodeKind for HtmlBlock {
    fn typ(&self) -> NodeType {
        NodeType::LeafBlock
    }

    fn kind_name(&self) -> &'static str {
        "HtmlBlock"
    }

    fn is_atomic(&self) -> bool {
        true
    }
}

impl PrettyPrint for HtmlBlock {
    fn pretty_print(&self, w: &mut dyn Write, _source: &str, level: usize) -> fmt::Result {
        writeln!(w, "{}Type: {}", pp_indent(level), self.block_type())
    }
}

impl From<HtmlBlock> for KindData {
    fn from(data: HtmlBlock) -> Self {
        KindData::HtmlBlock(data)
    }
}

//   }}} HtmlBlock

// GFM {{{
//   Table {{{

/// Alignment of table cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TableCellAlignment {
    Left,
    Center,
    Right,
    None,
}

impl TableCellAlignment {
    /// Returns the string representation of the alignment.
    pub fn as_str(&self) -> &'static str {
        match self {
            TableCellAlignment::Left => "left",
            TableCellAlignment::Center => "center",
            TableCellAlignment::Right => "right",
            TableCellAlignment::None => "none",
        }
    }
}

/// Represents a table node.
#[derive(Debug, Default)]
pub struct Table {}

impl Table {
    /// Creates a new [`Table`] node.
    pub fn new() -> Self {
        Self {}
    }
}

impl NodeKind for Table {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "Table"
    }
}

impl PrettyPrint for Table {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<Table> for KindData {
    fn from(e: Table) -> Self {
        KindData::Table(e)
    }
}

/// Represents a table row node.
#[derive(Debug, Default)]
pub struct TableRow {}

impl TableRow {
    /// Creates a new [`TableRow`] node.
    pub fn new() -> Self {
        Self {}
    }
}

impl NodeKind for TableRow {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "TableRow"
    }
}

impl PrettyPrint for TableRow {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<TableRow> for KindData {
    fn from(e: TableRow) -> Self {
        KindData::TableRow(e)
    }
}

/// Represents a table header node.
#[derive(Debug, Default)]
pub struct TableHeader {}

impl TableHeader {
    /// Creates a new [`TableHeader`] node.
    pub fn new() -> Self {
        Self {}
    }
}

impl NodeKind for TableHeader {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "TableHeader"
    }
}

impl PrettyPrint for TableHeader {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<TableHeader> for KindData {
    fn from(e: TableHeader) -> Self {
        KindData::TableHeader(e)
    }
}

/// Represents a table body node.
#[derive(Debug, Default)]
pub struct TableBody {}

impl TableBody {
    /// Creates a new [`TableBody`] node.
    pub fn new() -> Self {
        Self {}
    }
}

impl NodeKind for TableBody {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "TableBody"
    }
}

impl PrettyPrint for TableBody {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<TableBody> for KindData {
    fn from(e: TableBody) -> Self {
        KindData::TableBody(e)
    }
}

/// Represents a table cell node.
#[derive(Debug)]
pub struct TableCell {
    alignment: TableCellAlignment,
}

impl Default for TableCell {
    fn default() -> Self {
        Self {
            alignment: TableCellAlignment::None,
        }
    }
}

impl TableCell {
    /// Creates a new [`TableCell`] node with no alignment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new [`TableCell`] with the given alignment.
    pub fn with_alignment(alignment: TableCellAlignment) -> Self {
        Self { alignment }
    }

    /// Returns the alignment of the table cell.
    #[inline(always)]
    pub fn alignment(&self) -> TableCellAlignment {
        self.alignment
    }
}

impl NodeKind for TableCell {
    fn typ(&self) -> NodeType {
        NodeType::ContainerBlock
    }

    fn kind_name(&self) -> &'static str {
        "TableCell"
    }
}

impl PrettyPrint for TableCell {
    fn pretty_print(&self, w: &mut dyn Write, _source: &str, level: usize) -> fmt::Result {
        writeln!(w, "{}Alignment: {:?}", pp_indent(level), self.alignment,)
    }
}

impl From<TableCell> for KindData {
    fn from(e: TableCell) -> Self {
        KindData::TableCell(e)
    }
}

//   }}} Table
// }}} GFM

// }}} Blocks

// Inlines {{{
//   Text {{{

bitflags! {
    /// Qualifiers for textual nodes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct TextQualifier: u16 {
        /// Indicates given text has soft line break at the end.
        const SOFT_LINE_BREAK = 1 << 0;

        /// Indicates given text has hard line break at the end.
        const HARD_LINE_BREAK = 1 << 1;

        /// Indicates given text should be rendered without unescaping
        /// back slash escapes and resolving references.
        const RAW = 1 << 2;

        /// Indicates given text should be rendered without any
        /// modifications, such as escaping HTML characters.
        const CODE = 1 << 3;

        /// Indicates given text is temporary and might be removed
        /// later during processing.
        const TEMP = 1 << 4;
    }
}

/// Represents the textual content.
#[derive(Debug)]
#[non_exhaustive]
pub enum Textual {
    Segment(text::Segment),
    String(String),
}

impl From<text::Segment> for Textual {
    fn from(seg: text::Segment) -> Textual {
        Textual::Segment(seg)
    }
}

impl<T> From<T> for Textual
where
    T: Into<String>,
{
    fn from(t: T) -> Textual {
        Textual::String(t.into())
    }
}

/// Represents a text node in the document.
#[derive(Debug)]
pub struct Text {
    textual: Textual,

    qualifiers: TextQualifier,
}

impl Text {
    /// Creates a new [`Text`] node with the given textual content.
    pub fn new(textual: impl Into<Textual>) -> Self {
        let qualifiers = TextQualifier::default();
        Self {
            textual: textual.into(),
            qualifiers,
        }
    }

    /// Creates a new [`Text`] node with the given textual content and qualifiers.
    pub fn with_qualifiers(textual: impl Into<Textual>, qualifiers: TextQualifier) -> Self {
        Self {
            textual: textual.into(),
            qualifiers,
        }
    }

    /// The segment of the text.
    /// If the text is created from a string, returns None.
    #[inline(always)]
    pub fn segment(&self) -> Option<&text::Segment> {
        match self.textual {
            Textual::Segment(ref seg) => Some(seg),
            Textual::String(_) => None,
        }
    }

    /// Sets the textual content of this text.
    #[inline(always)]
    pub fn set(&mut self, textual: impl Into<Textual>) {
        self.textual = textual.into();
    }

    /// Adds the qualifiers to this text.
    pub fn add_qualifiers(&mut self, qualifiers: TextQualifier) {
        self.qualifiers |= qualifiers;
    }

    /// Returns true if this text has given qualifiers.
    pub fn has_qualifiers(&self, qualifiers: TextQualifier) -> bool {
        self.qualifiers.contains(qualifiers)
    }

    /// Returns the bytes of this text.
    pub fn bytes<'a>(&'a self, source: &'a str) -> Cow<'a, [u8]> {
        match &self.textual {
            Textual::Segment(seg) => seg.bytes(source),
            Textual::String(s) => Cow::Borrowed(s.as_bytes()),
        }
    }

    /// Returns the UTF-8 string  of this text.
    pub fn str<'a>(&'a self, source: &'a str) -> Cow<'a, str> {
        match &self.textual {
            Textual::Segment(seg) => seg.str(source),
            Textual::String(s) => Cow::Borrowed(s.as_str()),
        }
    }
}

impl NodeKind for Text {
    fn typ(&self) -> NodeType {
        NodeType::Inline
    }

    fn kind_name(&self) -> &'static str {
        "Text"
    }

    fn is_atomic(&self) -> bool {
        true
    }
}

impl PrettyPrint for Text {
    fn pretty_print(&self, w: &mut dyn Write, source: &str, level: usize) -> fmt::Result {
        writeln!(w, "{}Qualifiers: {:?}", pp_indent(level), self.qualifiers)?;
        writeln!(w, "{}Content: '{}'", pp_indent(level), self.str(source))
    }
}

impl From<Text> for KindData {
    fn from(data: Text) -> Self {
        KindData::Text(data)
    }
}

//   }}} Text

//   CodeSpan {{{

/// Represents an inline code span in the document.
#[derive(Debug, Default)]
pub struct CodeSpan {}

impl CodeSpan {
    /// Creates a new CodeSpan.
    pub fn new() -> Self {
        Self::default()
    }
}

impl NodeKind for CodeSpan {
    fn typ(&self) -> NodeType {
        NodeType::Inline
    }

    fn kind_name(&self) -> &'static str {
        "CodeSpan"
    }

    fn is_atomic(&self) -> bool {
        true
    }
}

impl PrettyPrint for CodeSpan {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<CodeSpan> for KindData {
    fn from(data: CodeSpan) -> Self {
        KindData::CodeSpan(data)
    }
}

//   }}} CodeSpan

//   Emphasis {{{

/// Represents an emphasis node in the document.
#[derive(Debug)]
pub struct Emphasis {
    level: u8,
}

impl Emphasis {
    /// Creates a new Emphasis.
    pub fn new(level: u8) -> Self {
        Self { level }
    }

    /// Returns the level of emphasis.
    #[inline(always)]
    pub fn level(&self) -> u8 {
        self.level
    }
}

impl NodeKind for Emphasis {
    fn typ(&self) -> NodeType {
        NodeType::Inline
    }

    fn kind_name(&self) -> &'static str {
        "Emphasis"
    }
}

impl PrettyPrint for Emphasis {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<Emphasis> for KindData {
    fn from(data: Emphasis) -> Self {
        KindData::Emphasis(data)
    }
}

//   }}} Emphasis

//   Link {{{

/// Represents a link in the document.
#[derive(Debug)]
pub struct Link {
    destination: text::Value,

    title: Option<text::Value>,

    auto_link_text: Option<text::Value>,
}

impl Link {
    /// Creates a new Link with the given destination and optional title.
    pub fn new(destination: impl Into<text::Value>, title: Option<impl Into<text::Value>>) -> Self {
        Self {
            destination: destination.into(),
            title: title.map(|t| t.into()),
            auto_link_text: None,
        }
    }

    /// Creates a new auto link with the given destination and auto link text.
    pub fn auto(destination: impl Into<text::Value>, text: impl Into<text::Value>) -> Self {
        Self {
            destination: destination.into(),
            title: None,
            auto_link_text: Some(text.into()),
        }
    }

    /// Returns the destination of the link.
    #[inline(always)]
    pub fn destination(&self) -> &text::Value {
        &self.destination
    }

    /// Returns the title of the link, if it exists.
    #[inline(always)]
    pub fn title(&self) -> Option<&text::Value> {
        self.title.as_ref()
    }

    /// Returns the auto link text of the link, if this is an auto link.
    #[inline(always)]
    pub fn auto_link_text(&self) -> Option<&text::Value> {
        self.auto_link_text.as_ref()
    }

    /// Returns true if this link is an auto link.
    pub fn is_auto_link(&self) -> bool {
        self.auto_link_text.is_some()
    }
}

impl NodeKind for Link {
    fn typ(&self) -> NodeType {
        NodeType::Inline
    }

    fn kind_name(&self) -> &'static str {
        "Link"
    }
}

impl PrettyPrint for Link {
    fn pretty_print(&self, w: &mut dyn Write, source: &str, level: usize) -> fmt::Result {
        if let Some(auto_text) = &self.auto_link_text {
            writeln!(w, "{}AutoLink: true", pp_indent(level),)?;
            writeln!(
                w,
                "{}AutoLinkText: {}",
                pp_indent(level),
                auto_text.str(source)
            )?;
        }
        writeln!(
            w,
            "{}Destination: {}",
            pp_indent(level),
            self.destination.str(source)
        )?;
        if let Some(title) = &self.title {
            writeln!(w, "{}Title: {}", pp_indent(level), title.str(source))?;
        }
        Ok(())
    }
}

impl From<Link> for KindData {
    fn from(data: Link) -> Self {
        KindData::Link(data)
    }
}

//   }}} Link

//   Image {{{

/// Represents an image in the document.
#[derive(Debug)]
pub struct Image {
    destination: text::Value,

    title: Option<text::Value>,
}

impl Image {
    /// Creates a new Image with the given destination and optional title.
    pub fn new(destination: impl Into<text::Value>, title: Option<impl Into<text::Value>>) -> Self {
        Self {
            destination: destination.into(),
            title: title.map(|t| t.into()),
        }
    }

    /// Returns the destination of the link.
    #[inline(always)]
    pub fn destination(&self) -> &text::Value {
        &self.destination
    }

    /// Returns the title of the link, if it exists.
    #[inline(always)]
    pub fn title(&self) -> Option<&text::Value> {
        self.title.as_ref()
    }
}

impl NodeKind for Image {
    fn typ(&self) -> NodeType {
        NodeType::Inline
    }

    fn kind_name(&self) -> &'static str {
        "Image"
    }
}

impl PrettyPrint for Image {
    fn pretty_print(&self, w: &mut dyn Write, source: &str, level: usize) -> fmt::Result {
        writeln!(
            w,
            "{}Destination: {}",
            pp_indent(level),
            self.destination.str(source)
        )?;
        if let Some(title) = &self.title {
            writeln!(w, "{}Title: {}", pp_indent(level), title.str(source))?;
        }
        Ok(())
    }
}

impl From<Image> for KindData {
    fn from(data: Image) -> Self {
        KindData::Image(data)
    }
}

//   }}} Image

//   RawHtml {{{

/// Represents an inline raw HTML node.
#[derive(Debug, Default)]
pub struct RawHtml {
    lines: Vec<text::Segment>,
}

impl RawHtml {
    /// Creates a new RawHtml
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the lines of the raw HTML.
    #[inline(always)]
    pub fn lines(&self) -> &text::Block {
        &self.lines
    }

    /// Adds a line to the raw HTML.
    #[inline(always)]
    pub fn add_line(&mut self, line: text::Segment) {
        self.lines.push(line);
    }
}

impl NodeKind for RawHtml {
    fn typ(&self) -> NodeType {
        NodeType::Inline
    }

    fn kind_name(&self) -> &'static str {
        "RawHtml"
    }

    fn is_atomic(&self) -> bool {
        true
    }
}

impl PrettyPrint for RawHtml {
    fn pretty_print(&self, w: &mut dyn Write, source: &str, level: usize) -> fmt::Result {
        writeln!(
            w,
            "{}RawText: {}",
            pp_indent(level),
            self.lines
                .iter()
                .map(|line| line.str(source))
                .collect::<String>()
        )?;
        Ok(())
    }
}

impl From<RawHtml> for KindData {
    fn from(data: RawHtml) -> Self {
        KindData::RawHtml(data)
    }
}

//   }}} RawHtml

// GFM {{{

/// Represents a strikethrough node.
#[derive(Debug, Default)]
pub struct Strikethrough {}

impl Strikethrough {
    /// Creates a new Strikethrough node.
    pub fn new() -> Self {
        Self {}
    }
}

impl NodeKind for Strikethrough {
    fn typ(&self) -> NodeType {
        NodeType::Inline
    }

    fn kind_name(&self) -> &'static str {
        "Strikethrough"
    }
}

impl PrettyPrint for Strikethrough {
    fn pretty_print(&self, _w: &mut dyn Write, _source: &str, _level: usize) -> fmt::Result {
        Ok(())
    }
}

impl From<Strikethrough> for KindData {
    fn from(e: Strikethrough) -> Self {
        KindData::Strikethrough(e)
    }
}
// }}} GFM

// }}} Inlines

// ExtensionData {{{

/// A composite trait for nodes will be added from outside of this library.
pub trait ExtensionData: Debug + PrettyPrint + NodeKind + Any {
    fn as_any(&self) -> &dyn Any;
}

impl<T: PrettyPrint + NodeKind + Debug + Any> ExtensionData for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// }}}
