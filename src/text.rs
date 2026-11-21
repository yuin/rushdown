//! Text related structures and traits.

extern crate alloc;

use alloc::{borrow::Cow, string::String, vec::Vec};
use core::{fmt::Debug, ops::Range};

use memchr::{memchr, memchr2};

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;
use crate::{
    error::Result,
    renderer::TextWrite,
    util::{
        self, is_blank, is_space, try_resolve_entity_reference, try_resolve_numeric_reference,
        try_unescape_punct, utf8_len, TinyVec, UnescapePunctResult,
    },
};

const SPACE: &[u8] = b" ";

// Decoder {{{

/// A Decoder decodes a byte slice, e.g. resolving backslash escapes and
/// character references.
///
/// Note that a decoder is not intended to 'normalize' a byte slice, but to decode it.
/// For example, CommonMark requires that code spans normalize leading spaces, trailing spaces
/// and newlines, but decoders do not do that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Decoder {
    /// Decodes character references and backslash escapes.
    ///
    /// CommonMark defines texts except some inline elements (e.g., code span, auto link, etc)
    /// can contain character references and backslash escapes.
    ///
    /// - decodes entity references (e.g., `&amp;` -> `&`)
    /// - decodes numeric character references (e.g., `&#x26;` -> `&`)
    /// - decodes `\` escaped punctuations (e.g., `\*` -> `*`)
    #[default]
    Default,

    /// Same as [`Decoder::Default`], but also decodes a halfspace escaped by backslash
    /// to an empty string.
    EscapedSpace,

    /// Does not perform any decoding and returns the bytes as is.
    Identity,
}

impl Decoder {
    /// Returns [`Decoder::EscapedSpace`] if `escaped_space` is true, otherwise
    /// [`Decoder::Default`].
    pub fn new(escaped_space: bool) -> Self {
        if escaped_space {
            Decoder::EscapedSpace
        } else {
            Decoder::Default
        }
    }

    /// Decodes the given byte slice and returns the decoded bytes.
    pub fn decode<'a>(&self, b: Cow<'a, [u8]>) -> Cow<'a, [u8]> {
        let escaped_space = match self {
            Decoder::Identity => return b,
            Decoder::Default => false,
            Decoder::EscapedSpace => true,
        };
        let Some(mut i) = memchr2(b'\\', b'&', &b) else {
            return b;
        };
        let bytes: &[u8] = &b;
        let mut result: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut n = 0usize;
        let mut changed = false;
        loop {
            let c = bytes[i];
            let mut next = i + 1;
            if c == b'\\' {
                match try_unescape_punct(bytes, i, escaped_space) {
                    UnescapePunctResult::Punct(nbyte, ch) => {
                        result.extend_from_slice(&bytes[n..i]);
                        result.push(ch);
                        next = i + nbyte + 1;
                        n = next;
                        changed = true;
                    }
                    UnescapePunctResult::Skipped(nbyte) => {
                        result.extend_from_slice(&bytes[n..i]);
                        next = i + nbyte + 1;
                        n = next;
                        changed = true;
                    }
                    UnescapePunctResult::None => {}
                }
            } else if let Some((nbyte, ch)) = try_resolve_numeric_reference(bytes, i) {
                result.extend_from_slice(&bytes[n..i]);
                let mut buf = [0u8; 4];
                result.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                next = i + nbyte + 1;
                n = next;
                changed = true;
            } else if let Some((nbyte, rep)) = try_resolve_entity_reference(bytes, i) {
                result.extend_from_slice(&bytes[n..i]);
                result.extend_from_slice(rep.as_bytes());
                next = i + nbyte + 1;
                n = next;
                changed = true;
            }
            match bytes.get(next..).and_then(|r| memchr2(b'\\', b'&', r)) {
                Some(rel) => i = next + rel,
                None => break,
            }
        }
        if !changed {
            return b;
        }
        result.extend_from_slice(&bytes[n..]);
        Cow::Owned(result)
    }

    /// Decodes the given byte slice and writes the decoded text to `w` in a single pass,
    /// without building an intermediate buffer.
    ///
    /// `b` must be valid UTF-8.
    pub fn decode_to<W: TextWrite + ?Sized>(&self, w: &mut W, b: &[u8]) -> Result<()> {
        // Chunks are split only at ASCII bytes, so they are valid UTF-8 if `b` is.
        fn chunk<W: TextWrite + ?Sized>(w: &mut W, b: &[u8]) -> Result<()> {
            if b.is_empty() {
                return Ok(());
            }
            w.write_str(unsafe { core::str::from_utf8_unchecked(b) })
        }
        let escaped_space = match self {
            Decoder::Identity => return chunk(w, b),
            Decoder::Default => false,
            Decoder::EscapedSpace => true,
        };
        let mut n = 0usize;
        let mut pos = 0usize;
        while let Some(rel) = memchr2(b'\\', b'&', &b[pos..]) {
            let i = pos + rel;
            pos = i + 1;
            if b[i] == b'\\' {
                match try_unescape_punct(b, i, escaped_space) {
                    UnescapePunctResult::Punct(nbyte, ch) => {
                        chunk(w, &b[n..i])?;
                        w.write_char(ch as char)?;
                        pos = i + nbyte + 1;
                        n = pos;
                    }
                    UnescapePunctResult::Skipped(nbyte) => {
                        chunk(w, &b[n..i])?;
                        pos = i + nbyte + 1;
                        n = pos;
                    }
                    UnescapePunctResult::None => {}
                }
            } else if let Some((nbyte, ch)) = try_resolve_numeric_reference(b, i) {
                chunk(w, &b[n..i])?;
                w.write_char(ch)?;
                pos = i + nbyte + 1;
                n = pos;
            } else if let Some((nbyte, rep)) = try_resolve_entity_reference(b, i) {
                chunk(w, &b[n..i])?;
                w.write_str(rep)?;
                pos = i + nbyte + 1;
                n = pos;
            }
        }
        chunk(w, &b[n..])
    }
}

// }}} Decoder

// Value {{{

/// A value for inline codes.
///
/// This value normalizes its content: line endings are converted to spaces.
/// Stripping a leading and a trailing space is the responsibility of the creator.
/// This value is never decoded.
#[derive(Debug, Clone)]
pub struct CodeSpanValue {
    indices: TinyVec<Index>,
    string: Option<String>,
}

impl CodeSpanValue {
    /// Creates a new [`CodeSpanValue`] backed by the source.
    pub fn from_indices(indices: impl Into<TinyVec<Index>>) -> Self {
        Self {
            indices: indices.into(),
            string: None,
        }
    }

    /// Creates a new [`CodeSpanValue`] backed by the given string.
    pub fn from_string(s: impl Into<String>) -> Self {
        Self {
            indices: TinyVec::empty(),
            string: Some(s.into()),
        }
    }

    /// Returns indices of this value if this value is backed by the source.
    pub fn indices(&self) -> Option<&[Index]> {
        match &self.string {
            Some(_) => None,
            None => Some(self.indices.as_slice()),
        }
    }

    /// Returns the source bytes of this value without normalization.
    pub fn bytes<'a>(&'a self, source: &'a str) -> Cow<'a, [u8]> {
        match &self.string {
            Some(s) => Cow::Borrowed(s.as_bytes()),
            None => match (self.indices.get(0), self.indices.get(1)) {
                (None, _) => Cow::Borrowed(&[]),
                (Some(f), None) => Cow::Borrowed(f.bytes(source)),
                _ => {
                    let mut v = Vec::new();
                    for idx in self.indices.iter() {
                        v.extend_from_slice(idx.bytes(source));
                    }
                    Cow::Owned(v)
                }
            },
        }
    }

    /// Returns the normalized string value.
    pub fn value<'a>(&'a self, source: &'a str) -> Cow<'a, str> {
        let bytes = self.bytes(source);
        if memchr(b'\n', &bytes).is_none() {
            return match bytes {
                Cow::Borrowed(b) => Cow::Borrowed(unsafe { core::str::from_utf8_unchecked(b) }),
                Cow::Owned(v) => Cow::Owned(unsafe { String::from_utf8_unchecked(v) }),
            };
        }
        let mut v = bytes.into_owned();
        for b in v.iter_mut() {
            if *b == b'\n' {
                *b = b' ';
            }
        }
        Cow::Owned(unsafe { String::from_utf8_unchecked(v) })
    }

    /// Returns true if the value is empty, otherwise false.
    pub fn is_empty(&self) -> bool {
        match &self.string {
            Some(s) => s.is_empty(),
            None => self.indices.iter().all(|i| i.is_empty()),
        }
    }

    /// Returns the length of the raw value in bytes.
    pub fn len(&self) -> usize {
        match &self.string {
            Some(s) => s.len(),
            None => self.indices.iter().map(|i| i.len()).sum(),
        }
    }
}

//   }}} ExtensionValue

//   Value {{{

/// An enum represents a string value that can be either an [`Index`], a collection of
/// [`Index`]s, or a [`String`]. [`Value::Empty`] represents the absence of a value.
/// [`Value`] does not handle padding.
///
/// [`Value::Index`], [`Value::Indices`] and [`Value::String`] have a [`Decoder`]:
///
/// - [`Value::bytes`] and [`Value::str`] return the value without decoding and normalization.
/// - [`Value::value`] returns the decoded and normalized value.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum Value {
    /// An Empty variant represents the absence of a value.
    #[default]
    Empty,

    /// An Index variant holds a reference to indicies in the source.
    Index(Index, Decoder),

    /// An Indices variant holds a collection of indices whose concatenation forms the value.
    Indices(TinyVec<Index>, Decoder),

    /// A String variant holds a string value.
    String(String, Decoder),

    /// A CodeSpan variant holds a value of an inline code. This value normalizes its content by
    /// itself and is never decoded.
    CodeSpan(CodeSpanValue),
}

impl Value {
    /// Creates a new value backed by the given index and decoder.
    pub fn from_index(index: Index, decoder: Decoder) -> Self {
        Value::Index(index, decoder)
    }

    /// Creates a new value backed by the given segment and decoder.
    pub fn from_segment(segment: Segment, decoder: Decoder) -> Self {
        Value::Index(Index::new(segment.start(), segment.stop()), decoder)
    }

    /// Creates a new value backed by the given indices and decoder.
    pub fn from_indices(indices: TinyVec<Index>, decoder: Decoder) -> Self {
        match (indices.get(0), indices.get(1)) {
            (Some(f), None) => Value::Index(*f, decoder),
            (None, _) => Value::Empty,
            _ => Value::Indices(indices, decoder),
        }
    }

    /// Creates a new value backed by the given string and decoder.
    pub fn from_string(s: impl Into<String>, decoder: Decoder) -> Self {
        Value::String(s.into(), decoder)
    }

    /// Returns the decoder of this value.
    /// Values that do not have a decoder return [`Decoder::Identity`].
    pub fn decoder(&self) -> Decoder {
        match self {
            Value::Index(_, d) | Value::Indices(_, d) | Value::String(_, d) => *d,
            _ => Decoder::Identity,
        }
    }

    /// Returns a new value that has the given decoder.
    /// Values that do not have a decoder are returned as they are.
    pub fn with_decoder(self, decoder: Decoder) -> Self {
        match self {
            Value::Index(i, _) => Value::Index(i, decoder),
            Value::Indices(i, _) => Value::Indices(i, decoder),
            Value::String(s, _) => Value::String(s, decoder),
            v => v,
        }
    }

    /// Returns byte slice value by concatenating all indices in the collection.
    /// The returned value is neither decoded nor normalized.
    pub fn bytes<'a>(&'a self, source: &'a str) -> Cow<'a, [u8]> {
        match self {
            Value::Empty => Cow::Borrowed(&[]),
            Value::Index(index, _) => Cow::Borrowed(index.bytes(source)),
            Value::Indices(indices, _) => match (indices.get(0), indices.get(1)) {
                (Some(f), None) => Cow::Borrowed(f.bytes(source)),
                (None, _) => Cow::Borrowed(&[]),
                _ => {
                    let mut result = Vec::new();
                    for idx in indices.iter() {
                        result.extend_from_slice(idx.bytes(source));
                    }
                    Cow::Owned(result)
                }
            },
            Value::String(s, _) => Cow::Borrowed(s.as_bytes()),
            Value::CodeSpan(c) => c.bytes(source),
        }
    }

    /// Returns str value by concatenating all indices in the collection.
    /// The returned value is neither decoded nor normalized.
    pub fn str<'a>(&'a self, source: &'a str) -> Cow<'a, str> {
        match self.bytes(source) {
            Cow::Borrowed(b) => Cow::Borrowed(unsafe { core::str::from_utf8_unchecked(b) }),
            Cow::Owned(v) => Cow::Owned(unsafe { String::from_utf8_unchecked(v) }),
        }
    }

    /// Returns the decoded and normalized bytes.
    pub fn value_bytes<'a>(&'a self, source: &'a str) -> Cow<'a, [u8]> {
        match self {
            Value::CodeSpan(c) => match c.value(source) {
                Cow::Borrowed(s) => Cow::Borrowed(s.as_bytes()),
                Cow::Owned(s) => Cow::Owned(s.into_bytes()),
            },
            _ => self.decoder().decode(self.bytes(source)),
        }
    }

    /// Returns the decoded and normalized string value.
    pub fn value<'a>(&'a self, source: &'a str) -> Cow<'a, str> {
        match self.value_bytes(source) {
            Cow::Borrowed(b) => Cow::Borrowed(unsafe { core::str::from_utf8_unchecked(b) }),
            Cow::Owned(v) => Cow::Owned(unsafe { String::from_utf8_unchecked(v) }),
        }
    }

    /// Writes the decoded and normalized value to `w` in a single pass.
    /// If `w` is an encoder (e.g. an HTML escaping writer), the value is decoded and encoded
    /// without any intermediate buffers.
    pub fn write_to<W: TextWrite + ?Sized>(&self, w: &mut W, source: &str) -> Result<()> {
        match self {
            Value::Empty => Ok(()),
            Value::Index(index, d) => d.decode_to(w, index.bytes(source)),
            Value::Indices(indices, d) => {
                for idx in indices.iter() {
                    d.decode_to(w, idx.bytes(source))?;
                }
                Ok(())
            }
            Value::String(s, d) => d.decode_to(w, s.as_bytes()),
            Value::CodeSpan(c) => w.write_str(&c.value(source)),
        }
    }

    /// Returns true if the value is empty, otherwise false.
    pub fn is_empty(&self) -> bool {
        match self {
            Value::Empty => true,
            Value::Index(index, _) => index.is_empty(),
            Value::Indices(indices, _) => indices.iter().all(|i| i.is_empty()),
            Value::String(s, _) => s.is_empty(),
            Value::CodeSpan(c) => c.is_empty(),
        }
    }

    /// Returns the length of the value.
    pub fn len(&self) -> usize {
        match self {
            Value::Empty => 0,
            Value::Index(index, _) => index.len(),
            Value::Indices(indices, _) => indices.iter().map(|i| i.len()).sum(),
            Value::String(s, _) => s.len(),
            Value::CodeSpan(c) => c.len(),
        }
    }

    /// Returns `None` if this value is empty, otherwise `Some(&self)`.
    pub fn as_option(&self) -> Option<&Value> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }

    /// Returns `None` if this value is empty, otherwise `Some(self)`.
    pub fn into_option(self) -> Option<Value> {
        if self.is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

impl From<&Value> for Value {
    fn from(v: &Value) -> Self {
        v.clone()
    }
}

impl From<CodeSpanValue> for Value {
    fn from(v: CodeSpanValue) -> Self {
        Value::CodeSpan(v)
    }
}
//   }}} Value

//   Index {{{

/// An Index struct holds information about source positions.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Index {
    start: usize,

    stop: usize,
}

impl Index {
    /// Create a new Index with start and stop.
    pub fn new(start: usize, stop: usize) -> Self {
        Index { start, stop }
    }

    /// A Start position of the index.
    #[inline(always)]
    pub fn start(&self) -> usize {
        self.start
    }

    /// A Stop position of the index.
    #[inline(always)]
    pub fn stop(&self) -> usize {
        self.stop
    }

    /// Returns the bytes of the index from the source.
    #[inline(always)]
    pub fn bytes<'a>(&self, source: &'a str) -> &'a [u8] {
        &source.as_bytes()[self.start..self.stop]
    }

    /// Returns the str of the index from the source.
    ///
    /// # Safety
    /// This method does not check the validity of UTF-8 boundaries.
    #[inline(always)]
    pub fn str<'a>(&self, source: &'a str) -> &'a str {
        unsafe { source.get_unchecked(self.start..self.stop) }
    }

    /// Returns true if the index is empty, otherwise false.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.stop
    }

    /// Returns a new Index with same value except `stop`.
    #[inline(always)]
    pub fn with_start(&self, v: usize) -> Index {
        Index::new(v, self.stop)
    }

    /// Returns a new Index with same value except `stop`.
    #[inline(always)]
    pub fn with_stop(&self, v: usize) -> Index {
        Index::new(self.start, v)
    }

    /// Returns the length of the index.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.stop - self.start
    }
}

impl From<(usize, usize)> for Index {
    fn from((start, stop): (usize, usize)) -> Self {
        Index::new(start, stop)
    }
}

impl From<Segment> for Index {
    fn from(segment: Segment) -> Self {
        Index::new(segment.start(), segment.stop())
    }
}

//   }}} Index

// }}} Value

// Segment {{{

//   Lines {{{

/// An enum represents a collection of segments that can be either a collection of [`Segment`] or
/// a single [`String`].
/// Lines is used for representing lines of block elements that can be represented by multiple lines,
/// such as HTML blocks.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum Lines {
    #[default]
    Empty,
    Segments(Vec<Segment>),
    String(String),
}

impl Lines {
    /// Creates a [`Lines`] from a collection of segments.
    pub fn from_segments(segments: Vec<Segment>) -> Self {
        Lines::Segments(segments)
    }

    /// Creates a [`Lines`] from a string.
    pub fn from_string(s: String) -> Self {
        Lines::String(s)
    }

    /// Returns an iterator that iterates over the lines of this [`Lines`] as str.
    pub fn iter<'a>(&'a self, source: &'a str) -> impl Iterator<Item = Cow<'a, str>> {
        LinesIter::new(
            match self {
                Lines::Empty => LinesIterState::Empty,
                Lines::Segments(segments) => LinesIterState::Segments(segments.iter()),
                Lines::String(s) => LinesIterState::String(s.split_inclusive('\n')),
            },
            source,
        )
    }
}

impl From<String> for Lines {
    fn from(s: String) -> Self {
        Lines::String(s)
    }
}

impl From<&String> for Lines {
    fn from(s: &String) -> Self {
        Lines::String(s.clone())
    }
}

impl From<&str> for Lines {
    fn from(s: &str) -> Self {
        Lines::String(String::from(s))
    }
}

impl From<&[u8]> for Lines {
    fn from(s: &[u8]) -> Self {
        Lines::String(String::from_utf8_lossy(s).into_owned())
    }
}

impl From<Vec<Segment>> for Lines {
    fn from(segments: Vec<Segment>) -> Self {
        Lines::Segments(segments)
    }
}

impl From<&[Segment]> for Lines {
    fn from(segments: &[Segment]) -> Self {
        Lines::Segments(segments.to_vec())
    }
}

enum LinesIterState<'a> {
    Empty,
    Segments(core::slice::Iter<'a, Segment>),
    String(core::str::SplitInclusive<'a, char>),
}

/// Iterator that iterates over [`Lines`] as str
struct LinesIter<'a> {
    state: LinesIterState<'a>,
    source: &'a str,
}

impl<'a> LinesIter<'a> {
    /// Creates a new LinesIter with the given lines and source.
    fn new(state: LinesIterState<'a>, source: &'a str) -> Self {
        LinesIter { state, source }
    }
}

impl<'a> Iterator for LinesIter<'a> {
    type Item = Cow<'a, str>;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.state {
            LinesIterState::Empty => None,
            LinesIterState::Segments(iter) => iter.next().map(|segment| segment.str(self.source)),
            LinesIterState::String(iter) => iter.next().map(Cow::Borrowed),
        }
    }
}

//   }}} Lines

//   Block {{{

/// Special collection of segments.
/// Each segment represents a one line.
/// Each segment does not contain multiple lines.
pub type Block = [Segment];

const fn binary_search_block_pos(block: &Block, pos: usize) -> Option<usize> {
    let mut left = 0;
    let mut right = block.len();
    while left < right {
        let mid = (left + right) / 2;
        if block[mid].start <= pos && pos < block[mid].stop {
            return Some(mid);
        }
        if pos < block[mid].start {
            right = mid;
        } else {
            left = mid + 1;
        }
    }
    None
}

/// Extension trait for [`Block`].
pub trait BlockExt {
    /// Returns a collection of values by converting each segment in the block to a value.
    fn to_values(&self, decoder: Decoder) -> Value;
}

impl BlockExt for Block {
    fn to_values(&self, decoder: Decoder) -> Value {
        block_to_values(self.iter().copied(), decoder)
    }
}

pub(crate) fn block_to_values(i: impl IntoIterator<Item = Segment>, decoder: Decoder) -> Value {
    let mut b = i.into_iter();
    let first = b.next();
    let second = b.next();
    if let Some(f) = first {
        if second.is_none() {
            return Value::from_segment(f, decoder);
        }
    } else {
        return Value::Empty;
    }
    let mut result = Vec::with_capacity(2 + b.size_hint().0);
    result.push(first.unwrap().into());
    result.push(second.unwrap().into());
    for segment in b {
        result.push(segment.into());
    }
    Value::Indices(TinyVec::from_vec(result), decoder)
}

//   }}} Block

//   Segment {{{

/// A Segment struct repsents a segment of CommonMark text.
/// In addition to [`Index`], Segment has padding and force_newline fields.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Segment {
    start: usize,

    stop: usize,

    padding: u8,

    force_newline: bool,
}

impl Segment {
    /// Creates a [`Segment`] with start and stop.
    pub fn new(start: usize, stop: usize) -> Self {
        Segment {
            start,
            stop,
            padding: 0,
            force_newline: false,
        }
    }

    /// Create a Segment with start, stop, and padding.
    pub fn new_with_padding(start: usize, stop: usize, padding: usize) -> Self {
        Segment {
            start,
            stop,
            padding: padding as u8,
            force_newline: false,
        }
    }

    /// A Start position of the segment.
    #[inline(always)]
    pub fn start(&self) -> usize {
        self.start
    }

    /// A Stop position of the segment.
    #[inline(always)]
    pub fn stop(&self) -> usize {
        self.stop
    }

    /// A Padding length of the segment.
    /// In CommonMark, Tab width is varied corresponding to horizontal position.
    /// So, padding is used to represent the number of leading spaces that should be inserted
    /// to align the text.
    #[inline(always)]
    pub fn padding(&self) -> usize {
        self.padding as usize
    }

    /// A Force newline flag of the segment.
    #[inline(always)]
    pub fn force_newline(&self) -> bool {
        self.force_newline
    }

    /// Returns the bytes of the segment from the source.
    pub fn bytes<'a>(&self, source: &'a str) -> Cow<'a, [u8]> {
        if self.padding == 0
            && (!self.force_newline || source.as_bytes().get(self.stop - 1) == Some(&b'\n'))
        {
            Cow::Borrowed(&source.as_bytes()[self.start..self.stop])
        } else {
            let mut result = Vec::with_capacity(self.padding() + self.stop - self.start + 1);
            result.extend(core::iter::repeat_n(SPACE[0], self.padding()));
            result.extend_from_slice(&source.as_bytes()[self.start..self.stop]);
            if self.force_newline && !result.is_empty() && *result.last().unwrap() != b'\n' {
                result.push(b'\n');
            }
            Cow::Owned(result)
        }
    }

    /// Returns the str of the segment from the source as a string.
    ///
    /// # Safety
    /// This method does not check the validity of UTF-8 boundaries.
    pub fn str<'a>(&self, source: &'a str) -> Cow<'a, str> {
        if self.padding == 0
            && (!self.force_newline || source.as_bytes().get(self.stop - 1) == Some(&b'\n'))
        {
            unsafe { Cow::Borrowed(source.get_unchecked(self.start..self.stop)) }
        } else {
            let mut result = String::with_capacity(self.padding() + self.stop - self.start + 1);
            result.extend(core::iter::repeat_n(' ', self.padding()));
            unsafe { result.push_str(source.get_unchecked(self.start..self.stop)) };
            if self.force_newline && !result.is_empty() && result.as_bytes().last() != Some(&b'\n')
            {
                result.push('\n');
            }
            Cow::Owned(result)
        }
    }

    /// Returns the length of the segment.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.stop - self.start + self.padding as usize
    }

    /// Returns a segment between this segment and the given segment.
    pub fn between(&self, other: Segment) -> Segment {
        if self.stop != other.stop {
            panic!("invalid state");
        }
        Segment::new_with_padding(
            self.start,
            other.start,
            (self.padding - other.padding) as usize,
        )
    }

    /// Returns true if this segment is empty, otherwise false.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.stop && self.padding == 0
    }

    /// Returns true if this segment is blank (only space characters), otherwise false.
    pub fn is_blank(&self, source: &str) -> bool {
        let v = &source.as_bytes()[self.start..self.stop];
        is_blank(v)
    }

    /// Returns a new segment by slicing off all trailing space characters.
    pub fn trim_right_space(&self, source: &str) -> Segment {
        let v = &source.as_bytes()[self.start..self.stop];
        let l = util::trim_right_space_length(v);
        if l == v.len() {
            Segment::new(self.start, self.start)
        } else {
            Segment::new_with_padding(self.start, self.stop - l, self.padding as usize)
        }
    }

    /// Returns a new segment by slicing off all leading space characters including padding.
    pub fn trim_left_space(&self, source: &str) -> Segment {
        let v = &source.as_bytes()[self.start..self.stop];
        let l = util::trim_left_space_length(v);
        Segment::new(self.start + l, self.stop)
    }

    /// Returns a new segment by slicing off leading space
    /// characters until the given width.
    pub fn trim_left_space_width(&self, mut width: isize, source: &str) -> Segment {
        let mut padding = self.padding as isize;
        while width > 0 && padding > 0 {
            width -= 1;
            padding -= 1;
        }
        if width == 0 {
            return Segment::new_with_padding(self.start, self.stop, padding as usize);
        }
        let v = &source.as_bytes()[self.start..self.stop];
        let mut start = self.start;
        for &c in v {
            if start >= self.stop - 1 || width == 0 {
                break;
            }
            if c == b' ' {
                width -= 1;
            } else if c == b'\t' {
                width -= 4;
            } else {
                break;
            }
            start += 1;
        }
        if width < 0 {
            padding = -width;
        }
        Segment::new_with_padding(start, self.stop, padding as usize)
    }

    /// Returns a new Segment with same value except `start`.
    #[inline(always)]
    pub fn with_start(&self, v: usize) -> Segment {
        Segment::new_with_padding(v, self.stop, self.padding as usize)
    }

    /// Returns a new Segment with same value except `stop`.
    #[inline(always)]
    pub fn with_stop(&self, v: usize) -> Segment {
        Segment::new_with_padding(self.start, v, self.padding as usize)
    }

    /// Returns a new Segment with padding set to given value.
    #[inline(always)]
    pub fn with_padding(&self, v: usize) -> Segment {
        Segment::new_with_padding(self.start, self.stop, v)
    }

    /// Returns a new Segment with force_newline set to `v`.
    #[inline(always)]
    pub fn with_force_newline(&self, v: bool) -> Segment {
        Segment {
            start: self.start,
            stop: self.stop,
            padding: self.padding,
            force_newline: v,
        }
    }

    /// Returns an Index with same start and stop as this segment.
    #[inline(always)]
    pub fn to_index(&self) -> Index {
        Index::new(self.start, self.stop)
    }
}

impl From<(usize, usize)> for Segment {
    fn from((start, stop): (usize, usize)) -> Self {
        Segment::new(start, stop)
    }
}

impl From<(usize, usize, usize)> for Segment {
    fn from((start, stop, padding): (usize, usize, usize)) -> Self {
        Segment::new_with_padding(start, stop, padding)
    }
}

impl From<Index> for Segment {
    fn from(index: Index) -> Self {
        Segment::new(index.start(), index.stop())
    }
}

impl From<Segment> for Range<usize> {
    fn from(segment: Segment) -> Self {
        segment.start()..segment.stop()
    }
}

//   }}} Segment

// }}} Segment

// Reader {{{

/// Indicates the end of string.
pub const EOS: u8 = 0xff;

/// A Reader trait represents a reader that can read and peek bytes.
pub trait Reader<'a> {
    /// Returns the source str.
    fn source(&self) -> &'a str;

    /// Returns the [`Decoder`] that will be bound to values created from this reader.
    fn decoder(&self) -> Decoder;

    /// Returns current line number and position.
    fn position(&self) -> (usize, Segment);

    /// Resets the internal pointer to the beginning of the source.
    fn reset_position(&mut self);

    /// Sets current line number and position.
    fn set_position(&mut self, line: usize, pos: Segment);

    /// Sets padding to the reader.
    fn set_padding(&mut self, padding: usize);

    /// Reads the next byte without advancing the position.
    /// Returns [`EOS`] if the end of the source is reached.
    fn peek_byte(&self) -> u8;

    /// Reads the next line segment without advancing the position.
    /// Returns None if the end of the source is reached.
    fn peek_line_segment(&self) -> Option<Segment>;

    /// Reads the next line without advancing the position.
    /// Returns None if the end of the source is reached.
    fn peek_line_bytes(&self) -> Option<(Cow<'a, [u8]>, Segment)>;

    /// Reads the next line without advancing the position.
    /// Returns None if the end of the source is reached.
    fn peek_line(&self) -> Option<(Cow<'a, str>, Segment)>;

    /// Advances the internal pointer.
    fn advance(&mut self, n: usize);

    /// Advances the internal pointer and add padding to the
    /// reader.
    fn advance_and_set_padding(&mut self, n: usize, padding: usize);

    /// Advances the internal pointer to the next line head.
    fn advance_line(&mut self);

    /// Advances the internal pointer to the end of line.
    /// If the line ends with a newline, it will be included in the segment.
    /// If the line ends with EOF, it will not be included in the segment.
    fn advance_to_eol(&mut self);

    /// Returns a distance from the line head to current position.
    fn line_offset(&mut self) -> usize;

    /// Returns a character just before current internal pointer.
    fn precending_charater(&self) -> char;

    /// Skips blank lines and advances the internal pointer to the next non-blank line.
    /// Returns None if the end of the source is reached.
    fn skip_blank_lines(&mut self) -> Option<(Cow<'a, [u8]>, Segment)> {
        loop {
            match self.peek_line_bytes() {
                None => return None,
                Some((line, seg)) => {
                    if is_blank(&line) {
                        self.advance_line();
                        continue;
                    }
                    return Some((line, seg));
                }
            }
        }
    }

    /// Skips bytes while the given function returns true.
    fn skip_while<F>(&mut self, mut f: F) -> usize
    where
        F: FnMut(u8) -> bool,
    {
        let mut i = 0usize;
        loop {
            let b = self.peek_byte();
            if b == EOS {
                break;
            }
            if f(b) {
                i += 1;
                self.advance(1);
                continue;
            }
            break;
        }
        i
    }

    /// Skips space characters.
    fn skip_spaces(&mut self) -> usize {
        self.skip_while(is_space)
    }
}

//   BasicReader {{{

/// [`Reader`] implementation for byte slices.
pub struct BasicReader<'a> {
    source: &'a str,
    bsource: &'a [u8],
    source_length: usize,
    line: Option<usize>,
    pos: Segment,
    head: usize,
    line_offset: Option<usize>,
    decoder: Decoder,
}

impl<'a> BasicReader<'a> {
    /// Creates a new BasicReader with the given source.
    pub fn new(source: &'a str) -> Self {
        Self::with_decoder(source, Decoder::Default)
    }

    /// Creates a new BasicReader with the given source and decoder.
    pub fn with_decoder(source: &'a str, decoder: Decoder) -> Self {
        let bsource: &[u8] = source.as_bytes();
        let source_length = bsource.len();
        let mut b = BasicReader {
            source,
            bsource,
            source_length,
            line: None,
            pos: Segment::new(0, 0),
            head: 0,
            line_offset: None,
            decoder,
        };
        b.reset_position();
        b
    }
}

impl<'a> Reader<'a> for BasicReader<'a> {
    fn source(&self) -> &'a str {
        self.source
    }

    fn decoder(&self) -> Decoder {
        self.decoder
    }

    fn position(&self) -> (usize, Segment) {
        (self.line.unwrap_or(0), self.pos)
    }

    fn reset_position(&mut self) {
        self.line = None;
        self.head = 0;
        self.line_offset = None;
        self.advance_line();
    }

    fn set_position(&mut self, line: usize, pos: Segment) {
        self.line = Some(line);
        self.pos = pos;
        self.head = pos.start;
        self.line_offset = None;
    }

    fn set_padding(&mut self, padding: usize) {
        self.pos.padding = padding as u8;
    }

    fn peek_byte(&self) -> u8 {
        if self.source_length == 0 {
            return EOS;
        }
        if self.pos.padding() != 0 {
            return SPACE[0];
        }
        if self.pos.start() < self.source_length {
            return self.bsource[self.pos.start()];
        }
        EOS
    }

    fn peek_line_segment(&self) -> Option<Segment> {
        if self.source_length == 0 {
            return None;
        }
        if self.pos.start() < self.source_length {
            return Some(self.pos);
        }
        None
    }

    fn peek_line_bytes(&self) -> Option<(Cow<'a, [u8]>, Segment)> {
        if self.source_length == 0 {
            return None;
        }
        if self.pos.start() < self.source_length {
            return Some((self.pos.bytes(self.source), self.pos));
        }
        None
    }

    fn peek_line(&self) -> Option<(Cow<'a, str>, Segment)> {
        if self.source_length == 0 {
            return None;
        }
        if self.pos.start() < self.source_length {
            return Some((self.pos.str(self.source), self.pos));
        }
        None
    }

    fn advance(&mut self, n: usize) {
        if self.source_length == 0 {
            return;
        }

        if n < self.pos.len() && self.pos.padding() == 0 {
            if let Some(offset) = self.line_offset {
                if memchr(b'\t', &self.bsource[self.pos.start..self.pos.start + n]).is_some() {
                    self.line_offset = None;
                } else {
                    self.line_offset = Some(offset + n);
                }
            }
            self.pos.start += n;
            return;
        }
        self.line_offset = None;
        let mut n = n;
        while n > 0 && self.pos.start < self.source_length {
            if self.pos.padding != 0 {
                self.pos.padding -= 1;
                n -= 1;
                continue;
            }
            if self.bsource[self.pos.start] == b'\n' {
                self.advance_line();
                n -= 1;
                continue;
            }

            self.pos.start += 1;
            n -= 1;
        }
    }

    fn advance_and_set_padding(&mut self, n: usize, padding: usize) {
        self.advance(n);
        if padding > self.pos.padding() {
            self.set_padding(padding);
        }
    }

    fn advance_line(&mut self) {
        self.line_offset = None;
        if self.source_length == 0 || self.pos.start >= self.source_length {
            return;
        }

        if self.line.is_some() {
            self.pos.start = self.pos.stop;
            if self.pos.start >= self.source_length {
                return;
            }
            self.pos.stop = self.source_length;
            if self.bsource[self.pos.start] != b'\n' {
                if let Some(i) = memchr(b'\n', &self.bsource[self.pos.start..]) {
                    self.pos.stop = self.pos.start + i + 1;
                }
            } else {
                self.pos.stop = self.pos.start + 1;
            }
            self.line = Some(self.line.unwrap() + 1);
        } else {
            if let Some(i) = memchr(b'\n', self.bsource) {
                self.pos = (0, i + 1).into();
            } else {
                self.pos = (0, self.source_length).into();
            }
            self.line = Some(0);
        }
        self.head = self.pos.start;
        self.pos.padding = 0;
    }

    fn advance_to_eol(&mut self) {
        if self.source_length == 0 || self.pos.start >= self.source_length {
            return;
        }

        self.line_offset = None;
        if let Some(i) = memchr(b'\n', &self.bsource[self.pos.start..]) {
            self.pos.start += i;
        } else {
            self.pos.start = self.source_length;
        }
        self.pos.padding = 0;
    }

    fn line_offset(&mut self) -> usize {
        if self.line_offset.is_none() {
            let mut v = 0;
            for i in self.head..self.pos.start {
                if self.bsource[i] == b'\t' {
                    v += util::tab_width(v);
                } else {
                    v += 1;
                }
            }
            v -= self.pos.padding();
            self.line_offset = Some(v);
        }
        self.line_offset.unwrap_or(0)
    }

    fn precending_charater(&self) -> char {
        if self.pos.padding() != 0 {
            return ' ';
        }
        if self.pos.start() == 0 {
            return '\n';
        }
        let mut i = self.pos.start() - 1;
        loop {
            if let Some(l) = utf8_len(self.bsource[i]) {
                if l == 1 {
                    return self.bsource[i] as char;
                }
                return str::from_utf8(&self.bsource[i..i + l])
                    .ok()
                    .and_then(|s| s.chars().next())
                    .unwrap_or('\u{FFFD}');
            }
            i -= 1;
            if i == 0 {
                break;
            }
        }
        '\u{FFFD}'
    }
}

//   }}} BasicReader

//   BlockReader {{{

/// [`Reader`] implementation for given blocks.
pub struct BlockReader<'a> {
    source: &'a str,
    bsource: &'a [u8],
    block: &'a Block,
    line: Option<usize>,
    pos: Segment,
    head: usize,
    last: usize,
    line_offset: Option<usize>,
    decoder: Decoder,
}

impl<'a> BlockReader<'a> {
    /// Creates a new BlockReader with the given source and block.
    pub fn new(source: &'a str, block: &'a Block) -> Self {
        Self::with_decoder(source, block, Decoder::Default)
    }

    /// Creates a new BlockReader with the given source, block and decoder.
    pub fn with_decoder(source: &'a str, block: &'a Block, decoder: Decoder) -> Self {
        let mut b = BlockReader {
            source,
            bsource: source.as_bytes(),
            block,
            line: None,
            pos: Segment::new(0, 0),
            head: 0,
            last: 0,
            line_offset: None,
            decoder,
        };
        b.reset(block);
        b
    }

    /// Resets the reader with given new block.
    pub fn reset(&mut self, lines: &'a Block) {
        self.block = lines;
        self.reset_position();
    }

    /// Returns Values that contains value between the current position and the given
    /// position.
    pub fn between_current(&mut self, line: usize, pos: Segment) -> Value {
        if line == self.line.unwrap_or(0) {
            let seg = self.block[line];
            if pos.start() >= seg.start() && self.pos.start() <= seg.stop() {
                return block_to_values(
                    BetweenBlockIterator::single(pos.start()..self.pos.start()),
                    self.decoder,
                );
            }
        }
        block_to_values(
            BetweenBlockIterator::multi(
                BlockReader {
                    source: self.source,
                    bsource: self.bsource,
                    block: self.block,
                    line: self.line,
                    pos: self.pos,
                    head: self.head,
                    last: self.last,
                    line_offset: self.line_offset,
                    decoder: self.decoder,
                },
                line,
                pos,
            ),
            self.decoder,
        )
    }

    /// Returns Values that contains segments between the given range.
    pub fn between(&self, range: Range<usize>) -> Value {
        let from_line = binary_search_block_pos(self.block, range.start).unwrap_or(0);
        let mut from_pos = self.block[from_line];
        if range.start >= from_pos.start() && range.end <= from_pos.stop() {
            return block_to_values(BetweenBlockIterator::single(range), self.decoder);
        }
        let to_line =
            binary_search_block_pos(self.block, range.end).unwrap_or(self.block.len() - 1);
        let mut to_pos = self.block[to_line];
        to_pos.start = range.end;
        from_pos.start = range.start;

        block_to_values(
            BetweenBlockIterator::multi(
                BlockReader {
                    source: self.source,
                    bsource: self.bsource,
                    block: self.block,
                    line: Some(to_line),
                    pos: to_pos,
                    head: 0,
                    last: 0,
                    line_offset: None,
                    decoder: self.decoder,
                },
                from_line,
                from_pos,
            ),
            self.decoder,
        )
    }
}

struct MultilineBetweenBlock<'a> {
    reader: BlockReader<'a>,
    start_line: usize,
    start_pos: Segment,
    current_line: usize,
    current_pos: Segment,
}

struct BetweenBlockIterator<'a> {
    multi: Option<MultilineBetweenBlock<'a>>,
    single: Option<Range<usize>>,
    done: bool,
}

impl<'a> BetweenBlockIterator<'a> {
    fn multi(mut reader: BlockReader<'a>, line: usize, pos: Segment) -> BetweenBlockIterator<'a> {
        let (current_line, current_pos) = reader.position();
        reader.set_position(line, pos);
        BetweenBlockIterator {
            multi: Some(MultilineBetweenBlock {
                reader,
                start_line: line,
                start_pos: pos,
                current_line,
                current_pos,
            }),
            single: None,
            done: false,
        }
    }

    fn single(range: Range<usize>) -> BetweenBlockIterator<'a> {
        BetweenBlockIterator {
            multi: None,
            single: Some(range),
            done: false,
        }
    }
}

impl<'a> Iterator for BetweenBlockIterator<'a> {
    type Item = Segment;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if let Some(s) = &self.single {
            self.done = true;
            return Some((s.start, s.end).into());
        }
        if let Some(m) = &mut self.multi {
            let (ln, _) = m.reader.position();
            let (_, segment) = m.reader.peek_line_bytes()?;
            let start = if ln == m.start_line {
                m.start_pos.start()
            } else {
                segment.start()
            };
            let stop = if ln == m.current_line {
                m.current_pos.start()
            } else {
                segment.stop()
            };
            let seg = Segment::new(start, stop);
            if ln == m.current_line {
                m.reader.advance(stop - start);
                self.done = true;
            }
            m.reader.advance_line();
            return Some(seg);
        }
        None
    }
}

impl<'a> Reader<'a> for BlockReader<'a> {
    fn source(&self) -> &'a str {
        self.source
    }

    fn decoder(&self) -> Decoder {
        self.decoder
    }

    fn position(&self) -> (usize, Segment) {
        (self.line.unwrap_or(0), self.pos)
    }

    fn reset_position(&mut self) {
        self.line = None;
        self.head = 0;
        self.last = 0;
        self.line_offset = None;
        self.pos.start = 0;
        self.pos.stop = 0;
        self.pos.padding = 0;
        self.pos.force_newline = false;
        if let Some(l) = self.block.last() {
            self.last = l.stop;
        }
        self.advance_line();
    }

    fn set_position(&mut self, line: usize, pos: Segment) {
        self.line_offset = None;
        self.line = Some(line);
        self.pos = pos;
        if line < self.block.len() {
            self.head = self.block[line].start;
        }
    }

    fn set_padding(&mut self, padding: usize) {
        self.line_offset = None;
        self.pos.padding = padding as u8;
    }

    fn peek_byte(&self) -> u8 {
        if self.bsource.is_empty() || self.block.is_empty() {
            return EOS;
        }
        if self.pos.padding() != 0 {
            return SPACE[0];
        }
        let l = self.line.unwrap();
        if self.pos.is_empty() {
            if l < self.block.len() - 1 {
                let next = &self.block[l + 1];
                if next.padding() != 0 {
                    return SPACE[0];
                }
                if next.start < self.bsource.len() {
                    return self.bsource[next.start];
                }
            }
            return EOS;
        } else if self.pos.start < self.bsource.len() {
            return self.bsource[self.pos.start];
        }
        EOS
    }

    fn peek_line_segment(&self) -> Option<Segment> {
        if self.bsource.is_empty() || self.block.is_empty() {
            return None;
        }
        let l = self.line.unwrap();
        if self.pos.is_empty() {
            if l < self.block.len() - 1 {
                let s = self.block[l + 1].start;
                if s < self.bsource.len() {
                    return Some(self.block[l + 1]);
                }
            }
            return None;
        } else if self.pos.start < self.bsource.len() {
            return Some(self.pos);
        }
        None
    }

    fn peek_line_bytes(&self) -> Option<(Cow<'a, [u8]>, Segment)> {
        if self.bsource.is_empty() || self.block.is_empty() {
            return None;
        }
        let l = self.line.unwrap();
        if self.pos.is_empty() {
            if l < self.block.len() - 1 {
                let s = self.block[l + 1].start;
                if s < self.bsource.len() {
                    return Some((self.block[l + 1].bytes(self.source), self.block[l + 1]));
                }
            }
            return None;
        } else if self.pos.start < self.bsource.len() {
            return Some((self.pos.bytes(self.source), self.pos));
        }
        None
    }

    fn peek_line(&self) -> Option<(Cow<'a, str>, Segment)> {
        if self.bsource.is_empty() || self.block.is_empty() {
            return None;
        }
        let l = self.line.unwrap();
        if self.pos.is_empty() {
            if l < self.block.len() - 1 {
                let s = self.block[l + 1].start;
                if s < self.bsource.len() {
                    return Some((self.block[l + 1].str(self.source), self.block[l + 1]));
                }
            }
            return None;
        } else if self.pos.start < self.bsource.len() {
            return Some((self.pos.str(self.source), self.pos));
        }
        None
    }

    fn advance(&mut self, n: usize) {
        if self.bsource.is_empty() || self.block.is_empty() {
            return;
        }
        if n < self.pos.len() && self.pos.padding() == 0 {
            if let Some(offset) = self.line_offset {
                if memchr(b'\t', &self.bsource[self.pos.start..self.pos.start + n]).is_some() {
                    self.line_offset = None;
                } else {
                    self.line_offset = Some(offset + n);
                }
            }
            self.pos.start += n;
            return;
        }
        self.line_offset = None;
        let mut n = n;
        while n > 0 && self.pos.start < self.last {
            if self.pos.padding != 0 {
                self.pos.padding -= 1;
                n -= 1;
                continue;
            }
            if self.pos.start >= self.pos.stop - 1 && self.pos.stop < self.last {
                self.advance_line();
                n -= 1;
                continue;
            }

            self.pos.start += 1;
            n -= 1;
        }
    }

    fn advance_and_set_padding(&mut self, n: usize, padding: usize) {
        self.advance(n);
        if padding > self.pos.padding() {
            self.set_padding(padding);
        }
    }

    fn advance_line(&mut self) {
        if self.bsource.is_empty() || self.block.is_empty() {
            return;
        }
        let l = match self.line {
            Some(l) => l + 1,
            None => 0,
        };
        if l < self.block.len() {
            self.set_position(l, self.block[l]);
        } else {
            self.pos.start = self.source().len();
            self.pos.stop = self.pos.start;
            self.pos.padding = 0;
        }
    }

    fn advance_to_eol(&mut self) {
        if self.bsource.is_empty() || self.block.is_empty() {
            return;
        }
        self.line_offset = None;
        let c = self.bsource[self.pos.stop - 1];
        if c == b'\n' {
            self.pos.start = self.pos.stop - 1;
        } else {
            self.pos.start = self.pos.stop;
        }
    }

    fn line_offset(&mut self) -> usize {
        if self.bsource.is_empty() || self.block.is_empty() {
            return 0;
        }
        if self.line_offset.is_none() {
            let mut v = 0;
            for i in self.head..self.pos.start {
                if self.bsource[i] == b'\t' {
                    v += util::tab_width(v);
                } else {
                    v += 1;
                }
            }
            v -= self.pos.padding();
            self.line_offset = Some(v);
        }
        self.line_offset.unwrap_or(0)
    }

    fn precending_charater(&self) -> char {
        if self.pos.padding() != 0 {
            return ' ';
        }
        if self.pos.start() == 0 {
            return '\n';
        }
        if self.block.is_empty() {
            return '\n';
        }
        let first_line = &self.block[0];
        if self.line.unwrap_or(0) == 0 && self.pos.start() <= first_line.start() {
            return '\n';
        }

        let mut i = self.pos.start() - 1;
        loop {
            if let Some(l) = utf8_len(self.bsource[i]) {
                if l == 1 {
                    return self.bsource[i] as char;
                }
                return str::from_utf8(&self.bsource[i..i + l])
                    .ok()
                    .and_then(|s| s.chars().next())
                    .unwrap_or('\u{FFFD}');
            }
            i -= 1;
            if i == 0 {
                break;
            }
        }
        if i == 0 {
            return '\n';
        }
        '\u{FFFD}'
    }
}
//   }}} BlockReader

// }}} Reader

// Tests {{{

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    #[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
    use crate::println;

    fn dec(d: Decoder, s: &str) -> String {
        String::from_utf8(d.decode(Cow::Borrowed(s.as_bytes())).into_owned()).unwrap()
    }

    #[test]
    fn test_decoder() {
        let d = Decoder::Default;
        assert_eq!(dec(d, "a \\* b &amp; &#35; &#x41; &copy;"), "a * b & # A ©");
        assert_eq!(dec(d, "\\&amp;"), "&amp;");
        assert_eq!(dec(d, "\\a &unknown; &#; \\"), "\\a &unknown; &#; \\");
        assert_eq!(dec(Decoder::Identity, "\\* &amp;"), "\\* &amp;");
        assert!(matches!(
            Decoder::Default.decode(Cow::Borrowed(b"plain")),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn test_decoder_pathological_input() {
        let s = "&".repeat(100_000) + &"\\".repeat(100_000);
        assert_eq!(dec(Decoder::Default, &s).len(), 150_000);
    }

    #[test]
    fn test_value_decoding() {
        let v = Value::from_string(String::from("a &amp; b"), Decoder::Default);
        assert_eq!(v.value(""), "a & b");
        assert_eq!(v.str(""), "a &amp; b");
        let v = Value::from_string(String::from("a &amp; b"), Decoder::Identity);
        assert_eq!(v.value(""), "a &amp; b");
        let v: Value = CodeSpanValue::from_string(String::from("a\nb &amp;")).into();
        assert_eq!(v.value(""), "a b &amp;");
    }

    #[test]
    fn test_segment() {
        let buffer = "Hello, world!";
        let segment: Segment = (0, 5).into();
        let s: &[u8] = &segment.bytes(buffer);
        assert_eq!(s, b"Hello");

        let segment_with_padding = Segment::new_with_padding(0, 5, 3);
        let s: &[u8] = &segment_with_padding.bytes(buffer);
        assert_eq!(s, b"   Hello");
    }

    #[test]
    fn test_raw() {
        let buffer = "Hello, world!";
        let index = Value::Index(Index::new(0, 5), Decoder::Default);
        assert_eq!(index.bytes(buffer), b"Hello".as_slice());

        let raw_string = Value::from_string(String::from("Hello"), Decoder::Default);
        assert_eq!(raw_string.bytes(buffer), b"Hello".as_slice());

        assert_eq!(index.str(buffer), "Hello");

        let string = String::from("Hello");
        let v = Value::from_string(string, Decoder::Default);
        assert_eq!(v.str(buffer), "Hello");
    }

    #[test]
    fn test_bytes_reader() {
        let buffer = "Hello, world!\nThis is a test.\n";
        let mut reader = BasicReader::new(buffer);
        assert_eq!(reader.peek_byte(), b'H');

        if let Some((line, segment)) = reader.peek_line_bytes() {
            assert_eq!(line.as_ref(), b"Hello, world!\n");
            assert_eq!(segment.start(), 0);
            assert_eq!(segment.stop(), 14);
        } else {
            panic!("Expected a line");
        }

        reader.advance(7);
        assert_eq!(reader.peek_byte(), b'w');

        reader.advance_line();
        assert_eq!(reader.peek_byte(), b'T');

        if let Some((line, segment)) = reader.peek_line_bytes() {
            assert_eq!(line.as_ref(), b"This is a test.\n");
            assert_eq!(segment.start(), 14);
            assert_eq!(segment.stop(), 30);
        } else {
            panic!("Expected a line");
        }

        reader.advance(100); // Advance beyond the end
        assert_eq!(reader.peek_byte(), EOS);
        assert!(reader.peek_line_bytes().is_none());
    }

    #[test]
    fn test_basic_reader_line_offset() {
        let buffer = "ab\tcd\nxyz\n";
        let mut reader = BasicReader::new(buffer);
        assert_eq!(reader.line_offset(), 0);
        assert_eq!(reader.line_offset, Some(0));

        // No tab in the advanced range: the cached offset is incremented.
        reader.advance(2);
        assert_eq!(reader.line_offset, Some(2));
        assert_eq!(reader.line_offset(), 2);

        // Crossing a tab invalidates the cached offset; the recomputed value
        // is the visual column after the tab.
        reader.advance(1);
        assert_eq!(reader.line_offset, None);
        assert_eq!(reader.line_offset(), 2 + util::tab_width(2));
        assert_eq!(reader.line_offset, Some(2 + util::tab_width(2)));

        // Subsequent tab-free advances keep the offset cached.
        reader.advance(2);
        assert_eq!(reader.line_offset, Some(6));

        // Advancing across the line break takes the slow path and resets the
        // offset to the new line's head.
        reader.advance(1);
        assert_eq!(reader.line_offset, None);
        assert_eq!(reader.line_offset(), 0);
    }

    #[test]
    fn test_bytes_reader_empty() {
        let buffer = "";
        let mut reader = BasicReader::new(buffer);
        assert_eq!(reader.peek_byte(), EOS);
        assert!(reader.peek_line_bytes().is_none());
        reader.advance(10);
        assert_eq!(reader.peek_byte(), EOS);
        assert!(reader.peek_line_bytes().is_none());
        reader.advance_line();
        assert_eq!(reader.peek_byte(), EOS);
        assert!(reader.peek_line_bytes().is_none());
    }

    #[test]
    fn test_block_reader() {
        let buffer = "Hello, world!\nThis is a test.\n";
        let lines = [Segment::new(0, 14), Segment::new_with_padding(14, 30, 2)];
        let mut reader = BlockReader::new(buffer, &lines);
        assert_eq!(reader.peek_byte(), b'H');

        if let Some((line, segment)) = reader.peek_line_bytes() {
            assert_eq!(line.as_ref(), b"Hello, world!\n");
            assert_eq!(segment.start(), 0);
            assert_eq!(segment.stop(), 14);
        } else {
            panic!("Expected a line");
        }

        reader.advance(13);
        assert_eq!(reader.peek_byte(), b'\n');

        reader.advance(1);
        assert_eq!(reader.peek_byte(), SPACE[0]);

        if let Some((line, segment)) = reader.peek_line_bytes() {
            assert_eq!(line.as_ref(), b"  This is a test.\n");
            assert_eq!(segment.start(), 14);
            assert_eq!(segment.stop(), 30);
            assert_eq!(segment.padding(), 2);
        } else {
            panic!("Expected a line");
        }

        reader.advance(3);
        assert_eq!(reader.peek_byte(), b'h');

        reader.advance(100); // Advance beyond the end
        assert_eq!(reader.peek_byte(), EOS);
        assert!(reader.peek_line_bytes().is_none());
    }

    #[test]
    fn test_block_reader_line_offset() {
        let buffer = "ab\tcd\nxyz\n";
        let lines = [Segment::new(0, 6), Segment::new(6, 10)];
        let mut reader = BlockReader::new(buffer, &lines);
        assert_eq!(reader.line_offset(), 0);
        assert_eq!(reader.line_offset, Some(0));

        // No tab in the advanced range: the cached offset is incremented.
        reader.advance(2);
        assert_eq!(reader.line_offset, Some(2));
        assert_eq!(reader.line_offset(), 2);

        // Crossing a tab invalidates the cached offset; the recomputed value
        // is the visual column after the tab.
        reader.advance(1);
        assert_eq!(reader.line_offset, None);
        assert_eq!(reader.line_offset(), 2 + util::tab_width(2));
        assert_eq!(reader.line_offset, Some(2 + util::tab_width(2)));

        // Subsequent tab-free advances keep the offset cached.
        reader.advance(2);
        assert_eq!(reader.line_offset, Some(6));

        // Advancing across the line break takes the slow path and resets the
        // offset to the new line's head.
        reader.advance(1);
        assert_eq!(reader.line_offset, None);
        assert_eq!(reader.line_offset(), 0);
    }

    #[test]
    fn test_block_reader_empty() {
        let buffer = "";
        let lines: [Segment; 0] = [];
        let mut reader = BlockReader::new(buffer, &lines);
        assert_eq!(reader.peek_byte(), EOS);
        assert!(reader.peek_line_bytes().is_none());
        reader.advance(10);
        assert_eq!(reader.peek_byte(), EOS);
        assert!(reader.peek_line_bytes().is_none());
        reader.advance_line();
        assert_eq!(reader.peek_byte(), EOS);
        assert!(reader.peek_line_bytes().is_none());
    }
}

// }}} Tests
