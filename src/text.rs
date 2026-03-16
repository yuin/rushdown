//! Text related structures and traits.

extern crate alloc;

use memchr::memchr;

use crate::util::{self, is_blank, is_space, utf8_len};
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;

const SPACE: &[u8] = b" ";

// Value {{{

/// An enum represents a string value that can be either an [`Index`] or a [`String`].
/// [`Value`] does not handle padding and new lines.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Value {
    /// An Index variant holds a reference to indicies in the source.
    Index(Index),

    /// A String variant holds a string value.
    String(String),
}

impl Value {
    /// Returns byte slice value.
    pub fn bytes<'a>(&'a self, source: &'a str) -> &'a [u8] {
        match self {
            Value::Index(index) => index.bytes(source),
            Value::String(s) => s.as_bytes(),
        }
    }

    /// Returns str value.
    pub fn str<'a>(&'a self, source: &'a str) -> &'a str {
        match self {
            Value::Index(index) => index.str(source),
            Value::String(s) => s.as_str(),
        }
    }

    /// Returns true if the value is empty, otherwise false.
    pub fn is_empty(&self) -> bool {
        match self {
            Value::Index(index) => index.is_empty(),
            Value::String(s) => s.is_empty(),
        }
    }

    /// Returns the length of the value.
    pub fn len(&self) -> usize {
        match self {
            Value::Index(index) => index.len(),
            Value::String(s) => s.len(),
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(String::from(s))
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<&[u8]> for Value {
    fn from(s: &[u8]) -> Self {
        Value::String(String::from_utf8_lossy(s).into_owned())
    }
}

impl From<Vec<u8>> for Value {
    fn from(s: Vec<u8>) -> Self {
        Value::String(String::from_utf8_lossy(&s).into_owned())
    }
}

impl From<&[char]> for Value {
    fn from(s: &[char]) -> Self {
        Value::String(s.iter().collect())
    }
}

impl From<(usize, usize)> for Value {
    fn from((start, stop): (usize, usize)) -> Self {
        Value::Index(Index::new(start, stop))
    }
}

impl From<Segment> for Value {
    fn from(segment: Segment) -> Self {
        Value::Index(Index::new(segment.start(), segment.stop()))
    }
}

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
    pub fn bytes<'a>(&'a self, source: &'a str) -> &'a [u8] {
        &source.as_bytes()[self.start..self.stop]
    }

    /// Returns the str of the index from the source.
    ///
    /// # Safety
    /// This method does not check the validity of UTF-8 boundaries.
    #[inline(always)]
    pub fn str<'a>(&'a self, source: &'a str) -> &'a str {
        unsafe { source.get_unchecked(self.start..self.stop) }
    }

    /// Returns true if the index is empty, otherwise false.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
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
    pub fn len(&self) -> usize {
        self.stop - self.start
    }
}

impl From<Index> for Value {
    fn from(index: Index) -> Self {
        Value::Index(index)
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

/// Special collection of segments.
/// Each segment represents a one line.
/// Each segment does not contain multiple lines.
pub type Block = [Segment];

/// Converts a [`Block`] to a Value.
pub fn block_to_value(i: impl IntoIterator<Item = Segment>, source: &str) -> Value {
    let mut b = i.into_iter();
    let first = b.next();
    let second = b.next();
    if let Some(f) = first {
        if second.is_none() {
            return f.into();
        }
    } else {
        return Value::String(String::new());
    }
    let mut result = String::new();
    result.push_str(&first.unwrap().str(source));
    result.push_str(&second.unwrap().str(source));

    for segment in b {
        result.push_str(&segment.str(source));
    }
    Value::String(result)
}

/// Converts a [`Block`] to a bytes.
pub fn block_to_bytes<'a>(i: impl IntoIterator<Item = Segment>, source: &'a str) -> Cow<'a, [u8]> {
    let mut b = i.into_iter();
    let first = b.next();
    let second = b.next();
    if let Some(f) = first {
        if second.is_none() {
            return f.bytes(source);
        }
    } else {
        return Cow::Borrowed(&[]);
    }
    let mut result = Vec::new();
    result.extend_from_slice(&first.unwrap().bytes(source));
    result.extend_from_slice(&second.unwrap().bytes(source));

    for segment in b {
        result.extend_from_slice(&segment.bytes(source));
    }
    Cow::Owned(result)
}

/// Converts a [`Block`] to a str.
pub fn block_to_str<'a>(i: impl IntoIterator<Item = Segment>, source: &'a str) -> Cow<'a, str> {
    let mut b = i.into_iter();
    let first = b.next();
    let second = b.next();
    if let Some(f) = first {
        if second.is_none() {
            return f.str(source);
        }
    } else {
        return Cow::Borrowed("");
    }
    let mut result = String::new();
    result.push_str(&first.unwrap().str(source));
    result.push_str(&second.unwrap().str(source));

    for segment in b {
        result.push_str(&segment.str(source));
    }
    Cow::Owned(result)
}

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

    /// Create a Segment with start, stop, and padding.
    pub fn new_with_padding(start: usize, stop: usize, padding: usize) -> Self {
        Segment {
            start,
            stop,
            padding: padding as u8,
            force_newline: false,
        }
    }

    /// Returns the bytes of the segment from the source.
    pub fn bytes<'a>(&self, source: &'a str) -> Cow<'a, [u8]> {
        if self.padding == 0 && !self.force_newline {
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
        if self.padding == 0 && !self.force_newline {
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
    pub fn len(&self) -> usize {
        self.stop - self.start + self.padding()
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
    pub fn is_empty(&self) -> bool {
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

// }}} Segment

// Reader {{{

/// Indicates the end of string.
pub const EOS: u8 = 0xff;

/// A Reader trait represents a reader that can read and peek bytes.
pub trait Reader<'a> {
    /// Returns the source str.
    fn source(&self) -> &'a str;

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
}

impl<'a> BasicReader<'a> {
    /// Creates a new BasicReader with the given source.
    pub fn new(source: &'a str) -> Self {
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
        };
        b.reset_position();
        b
    }

    /// Creates a new BasicReader with the given byte slice without UTF-8 validation.
    ///
    /// # Safety
    /// - The caller must ensure that the given byte slice is valid UTF-8.
    pub unsafe fn new_unchecked(source: &'a [u8]) -> Self {
        Self::new(core::str::from_utf8_unchecked(source))
    }
}

impl<'a> Reader<'a> for BasicReader<'a> {
    fn source(&self) -> &'a str {
        self.source
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

        self.line_offset = None;
        if n < self.pos.len() && self.pos.padding() == 0 {
            self.pos.start += n;
            return;
        }
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
}

impl<'a> BlockReader<'a> {
    /// Creates a new BlockReader with the given source and block.
    pub fn new(source: &'a str, block: &'a Block) -> Self {
        let mut b = BlockReader {
            source,
            bsource: source.as_bytes(),
            block,
            line: None,
            pos: Segment::new(0, 0),
            head: 0,
            last: 0,
            line_offset: None,
        };
        b.reset(block);
        b
    }

    /// Creates a new BlockReader with the given byte slice without UTF-8 validation.
    ///
    /// # Safety
    /// - The caller must ensure that the given byte slice is valid UTF-8.
    pub unsafe fn new_unchecked(source: &'a [u8], block: &'a Block) -> Self {
        Self::new(core::str::from_utf8_unchecked(source), block)
    }

    /// Resets the reader with given new block.
    pub fn reset(&mut self, lines: &'a Block) {
        self.block = lines;
        self.reset_position();
    }

    /// Returns an iterator that yields segments between the current position and the given
    /// position.
    pub fn between_current(
        &mut self,
        line: usize,
        pos: Segment,
    ) -> impl Iterator<Item = Segment> + 'a {
        BetweenBlockIterator::new(
            BlockReader {
                source: self.source,
                bsource: self.bsource,
                block: self.block,
                line: self.line,
                pos: self.pos,
                head: self.head,
                last: self.last,
                line_offset: self.line_offset,
            },
            line,
            pos,
        )
    }
}

struct BetweenBlockIterator<'a> {
    reader: BlockReader<'a>,
    start_line: usize,
    start_pos: Segment,
    current_line: usize,
    current_pos: Segment,
    done: bool,
}

impl<'a> BetweenBlockIterator<'a> {
    fn new(mut reader: BlockReader<'a>, line: usize, pos: Segment) -> BetweenBlockIterator<'a> {
        let (current_line, current_pos) = reader.position();
        reader.set_position(line, pos);
        BetweenBlockIterator {
            reader,
            start_line: line,
            start_pos: pos,
            current_line,
            current_pos,
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
        let (ln, _) = self.reader.position();
        let (_, segment) = self.reader.peek_line_bytes()?;
        let start = if ln == self.start_line {
            self.start_pos.start()
        } else {
            segment.start()
        };
        let stop = if ln == self.current_line {
            self.current_pos.start()
        } else {
            segment.stop()
        };
        let seg = Segment::new(start, stop);
        if ln == self.current_line {
            self.reader.advance(stop - start);
            self.done = true;
        }
        self.reader.advance_line();
        Some(seg)
    }
}

impl<'a> Reader<'a> for BlockReader<'a> {
    fn source(&self) -> &'a str {
        self.source
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
        self.line_offset = None;
        if n < self.pos.len() && self.pos.padding() == 0 {
            self.pos.start += n;
            return;
        }
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
        let index = Value::from((0, 5));
        let s: &[u8] = index.bytes(buffer);
        assert_eq!(s, b"Hello");

        let raw_string = Value::from("Hello");
        let s: &[u8] = raw_string.bytes(buffer);
        assert_eq!(s, b"Hello");

        let str: &str = index.str(buffer);
        assert_eq!(str, "Hello");

        let string = String::from("Hello");
        let v = Value::from(string.as_str());
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
