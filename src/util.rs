//! Utility functions and data structures.

extern crate alloc;

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::{self, Debug};
use core::ops::{DerefMut, Index, IndexMut};
use core::{
    borrow::Borrow,
    cmp::{min, Ordering},
    ops::Deref,
};

#[cfg(any(feature = "hashbrown", not(feature = "std")))]
pub type HashMap<K, V> = hashbrown::HashMap<K, V>;

#[cfg(all(not(feature = "hashbrown"), feature = "std"))]
pub type HashMap<K, V> = std::collections::HashMap<K, V>;

#[cfg(any(feature = "hashbrown", not(feature = "std")))]
pub type HashSet<K> = hashbrown::HashSet<K>;

#[cfg(all(not(feature = "hashbrown"), feature = "std"))]
pub type HashSet<K> = std::collections::HashSet<K>;

include!(concat!(env!("OUT_DIR"), "/unicode_case_foldings.rs"));

// String utilities {{{

//   CowByteBuffer {{{

/// A copy-on-write byte buffer for efficient byte-level modifications.
pub struct CowByteBuffer<'a> {
    data: Cow<'a, [u8]>,
    buffer: Option<Vec<u8>>,
    last_pos: usize,
    i: usize,
    len: usize,
    inc: u8,
}

impl<'a> CowByteBuffer<'a> {
    /// Creates a new [`CowByteBuffer`] from the given byte slice.
    // pub fn new(s: impl In&'a [u8]) -> Self {
    pub fn new(s: impl Into<Cow<'a, [u8]>>) -> Self {
        let mut buf = Self {
            data: s.into(),
            buffer: None,
            last_pos: 0,
            i: 0,
            len: 0,
            inc: 0,
        };
        buf.len = buf.data.len();
        buf
    }

    /// Returns the current position.
    #[inline(always)]
    pub fn pos(&self) -> usize {
        self.i
    }

    /// Peeks the byte at the given offset from the current position.
    pub fn peek_byte(&self, n: usize) -> Option<&u8> {
        if self.i + n >= self.len {
            return None;
        }
        Some(&self.data[self.i + n])
    }

    /// Writes a byte at the current position and skips the given number of bytes.
    pub fn write_byte(&mut self, byte: u8, skip: usize) {
        if self.buffer.is_none() {
            self.buffer = Some(Vec::with_capacity(self.len + 16));
        }
        let buf = self.buffer.as_mut().unwrap();
        buf.extend_from_slice(&self.data[self.last_pos..self.i]);
        buf.push(byte);
        self.i += skip;
        self.last_pos = self.i + 1;
    }

    /// Writes bytes at the current position and skips the given number of bytes.
    pub fn write_bytes(&mut self, bytes: &[u8], skip: usize) {
        if self.buffer.is_none() {
            self.buffer = Some(Vec::with_capacity(self.len + 16));
        }
        let buf = self.buffer.as_mut().unwrap();
        buf.extend_from_slice(&self.data[self.last_pos..self.i]);
        buf.extend_from_slice(bytes);
        self.i += skip;
        self.last_pos = self.i + 1;
    }

    /// Writes bytes from the last position to the current position and skips the given number of
    /// bytes.
    pub fn write(&mut self, skip: usize) {
        if self.buffer.is_none() {
            self.buffer = Some(Vec::with_capacity(self.len + 16));
        }
        let buf = self.buffer.as_mut().unwrap();
        buf.extend_from_slice(&self.data[self.last_pos..self.i]);
        self.i += skip;
        self.last_pos = self.i + 1;
    }

    /// Skips the given number of bytes.
    pub fn skip(&mut self, n: usize) {
        self.i += n;
    }

    /// Returns the next byte and advances the position.
    #[inline(always)]
    pub fn next_byte(&mut self) -> Option<&u8> {
        self.i += self.inc as usize;
        if self.i >= self.len {
            return None;
        }
        let b = &self.data[self.i];
        self.inc = 1;
        Some(b)
    }

    /// Finalizes and returns the resulting byte slice.
    /// If no modifications were made, returns a borrowed slice.
    /// Otherwise, returns an owned buffer.
    pub fn end(self) -> Cow<'a, [u8]> {
        if let Some(mut buf) = self.buffer {
            buf.extend_from_slice(&self.data[self.last_pos..self.len]);
            Cow::Owned(buf)
        } else {
            self.data
        }
    }
}

impl Deref for CowByteBuffer<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

//   }}} CowByteBuffer

//   {{{ AsciiWordSet

/// A set of (short) ASCII words organized by their lengths for efficient lookup.
#[derive(Debug, Default)]
pub struct AsciiWordSet {
    buckets: Vec<Vec<&'static str>>,
    words: &'static str,
}

/// Options for creating an [`AsciiWordSet`].
pub struct AsciiWordSetOptions {
    /// Bucket size. It is recommended to be equal to or larger than the maximum word length.
    pub bucket_size: usize,
}

impl Default for AsciiWordSetOptions {
    fn default() -> Self {
        Self { bucket_size: 32 }
    }
}

impl AsciiWordSet {
    /// Creates a new empty [`AsciiWordSet`] with default options.
    /// `words` to be inserted into the set, separated by `,`.
    /// Words must be sorted.
    pub fn new(words: &'static str) -> Self {
        Self::with_options(words, AsciiWordSetOptions::default())
    }

    /// Creates a new empty [`AsciiWordSet`] with the given options.
    pub fn with_options(words: &'static str, options: AsciiWordSetOptions) -> Self {
        let mut s = Self {
            buckets: vec![Vec::new(); options.bucket_size],
            words,
        };
        for word in words.split(',') {
            let len = min(word.len() - 1, s.buckets.len() - 1);
            let b = &mut s.buckets[len];
            b.push(word);
        }
        s
    }

    /// Returns the words in the set as a comma-separated string.
    /// Words are always sorted.
    pub fn words(&self) -> &'static str {
        self.words
    }

    /// Checks if the set contains the given word.
    pub fn contains(&self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }

        let len = min(s.len() - 1, self.buckets.len() - 1);
        self.buckets[len].binary_search_by(|&x| x.cmp(s)).is_ok()
    }
}

//   }}} AsciiWordSet

const SPACES: [u8; 7] = [b' ', b'\t', b'\n', b'\r', 0x0b, 0x0c, 0x0d];

const SPACE_TABLE: [i8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Returns true if the given character is a space, otherwise false.
#[inline(always)]
pub fn is_space(c: u8) -> bool {
    SPACE_TABLE[c as usize] == 1
}

const PUNCT_TABLE: [i8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1,
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1,
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

/// Returns true if the given character is a punctuation, otherwise false.
#[inline(always)]
pub fn is_punct(c: u8) -> bool {
    PUNCT_TABLE[c as usize] == 1
}

/// Visualizes space characters in the given byte slice.
/// This function is mainly for debugging purposes.
pub(crate) fn visualize_spaces(s: impl AsRef<[u8]>) -> String {
    let s = unsafe { core::str::from_utf8_unchecked(s.as_ref()) };
    let mut out = String::new();
    for c in s.chars() {
        match c {
            ' ' => out.push_str("[SPACE]"),
            '\t' => out.push_str("[TAB]"),
            '\n' => out.push_str("[NEWLINE]"),
            '\r' => out.push_str("[CR]"),
            '\x0b' => out.push_str("[VT]"),
            '\x0c' => out.push_str("[FF]"),
            '\x00' => out.push_str("[NUL]"),
            _ => out.push(c),
        }
    }
    out
}

/// Trims characters in `b` from the head of `s`.
#[inline(always)]
pub fn trim_left<'a>(s: &'a [u8], b: &[u8]) -> &'a [u8] {
    let i = s.iter().position(|&c| !b.contains(&c)).unwrap_or(s.len());
    &s[i..]
}

/// Trims characters in `b` from the tail of `s`.
#[inline(always)]
pub fn trim_right<'a>(s: &'a [u8], b: &[u8]) -> &'a [u8] {
    let i = s
        .iter()
        .rposition(|&c| !b.contains(&c))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    &s[..i]
}

/// Trims characters in `b` from the head of `s` and returns the length of trimmed characters.
#[inline(always)]
pub fn trim_left_length(s: &[u8], b: &[u8]) -> usize {
    s.len() - trim_left(s, b).len()
}

/// Trims characters in `b` from the tail of `s` and returns the length of trimmed characters.
#[inline(always)]
pub fn trim_right_length(s: &[u8], b: &[u8]) -> usize {
    s.len() - trim_right(s, b).len()
}

/// Trims space characters from the head of `s` and returns the length of trimmed characters.
#[inline(always)]
pub fn trim_left_space_length(s: &[u8]) -> usize {
    s.iter().take_while(|&&c| is_space(c)).count()
}

/// Trims space characters from the tail of `s` and returns the length of trimmed characters.
#[inline(always)]
pub fn trim_right_space_length(s: &[u8]) -> usize {
    s.iter().rev().take_while(|&&c| is_space(c)).count()
}

/// Trims space characters from the head of `s`.
#[inline(always)]
pub fn trim_left_space(s: &[u8]) -> &[u8] {
    trim_left(s, &SPACES)
}

/// Trims space characters from the tail of `s`.
#[inline(always)]
pub fn trim_right_space(s: &[u8]) -> &[u8] {
    trim_right(s, &SPACES)
}

/// Returns true if `s` is empty or consists of only space characters.
#[inline(always)]
pub fn is_blank(s: &[u8]) -> bool {
    s.is_empty() || s.iter().all(|&c| is_space(c))
}

/// Returns true if `s` ends with the given suffix.
#[inline(always)]
pub fn has_suffix(s: &[u8], suffix: &[u8]) -> bool {
    if s.len() < suffix.len() {
        return false;
    }
    &s[s.len() - suffix.len()..] == suffix
}

/// Returns the indent width and the number of bytes consumed for `s`.
pub fn indent_width(s: &[u8], current_pos: usize) -> (usize, usize) {
    let mut width = 0;
    let mut pos = 0;
    for &b in s {
        if b == b' ' {
            width += 1;
            pos += 1;
        } else if b == b'\t' {
            width += tab_width(current_pos + width);
            pos += 1;
        } else {
            break;
        }
    }
    (width, pos)
}

/// Returns the width of a tab at the given position.
pub fn tab_width(current_pos: usize) -> usize {
    4 - (current_pos % 4)
}

/// Searches an indent position with the given width for the given line.
/// If the line contains tab characters, paddings may be not zero.
/// currentPos==0 and width==2:
///
///  ```text
///  position: 0    1
///            [TAB]aaaa
///  width:    1234 5678
///  ```
///
/// width=2 is in the tab character. In this case, IndentPosition returns
/// (pos=1, padding=2).
#[inline]
pub fn indent_position(s: &[u8], current_pos: usize, width: usize) -> Option<(usize, usize)> {
    indent_position_padding(s, current_pos, 0, width)
}

/// Searches an indent position with the given width for the given line.
/// This function is mostly same as [`indent_position`] except this function
/// takes account into additional paddings.
pub fn indent_position_padding(
    s: &[u8],
    current_pos: usize,
    padding: usize,
    width: usize,
) -> Option<(usize, usize)> {
    if width == 0 {
        return Some((0, padding));
    }
    let mut w = 0;
    let mut i = 0;
    let l = s.len();
    let mut p = padding;
    while i < l {
        if p > 0 {
            p -= 1;
            w += 1;
            i += 1;
            continue;
        }
        if s[i] == b'\t' && w < width {
            w += tab_width(current_pos + w);
        } else if s[i] == b' ' && w < width {
            w += 1;
        } else {
            break;
        }
        i += 1;
    }
    if w >= width {
        Some((i - padding, w - width))
    } else {
        None
    }
}

/// The result of trying to unescape a punctuation.
pub(crate) enum UnescapePunctResult {
    Punct(usize, u8),
    Skipped(usize),
    None,
}

/// Tries to unescape a punctuation at the given position.
/// `s[start]` must be '\\' .
/// This function does not check `s[start]` is '\\' .
#[inline(always)]
pub(crate) fn try_unescape_punct(s: &[u8], pos: usize, escaped_space: bool) -> UnescapePunctResult {
    if pos + 1 >= s.len() {
        return UnescapePunctResult::None;
    }
    let next_char = s[pos + 1];
    if is_punct(next_char) {
        UnescapePunctResult::Punct(1, next_char)
    } else if escaped_space && next_char == b' ' {
        UnescapePunctResult::Skipped(1)
    } else {
        UnescapePunctResult::None
    }
}

/// Unescapes blackslash escaped punctuations.
/// If `escaped_space` is true, a halfspace escaped by backslash is ignored.
pub fn unescape_puncts<'a>(c: impl Into<Cow<'a, [u8]>>, escaped_space: bool) -> Cow<'a, [u8]> {
    let cw = c.into();
    if memchr::memchr(b'\\', cw.as_ref()).is_none() {
        return cw;
    }
    let mut cow = CowByteBuffer::new(cw);
    while let Some(&b) = cow.next_byte() {
        if b != b'\\' {
            continue;
        }

        match try_unescape_punct(&cow, cow.pos(), escaped_space) {
            UnescapePunctResult::Punct(nbyte, ch) => {
                cow.write_byte(ch, nbyte);
            }
            UnescapePunctResult::Skipped(nbyte) => {
                cow.write(nbyte);
            }
            UnescapePunctResult::None => {}
        }
    }
    cow.end()
}

/// Converts given bytes into a valid link reference string.
/// This performs unicode case folding, trims leading and trailing spaces,  converts into lower
/// case and replace spaces with a single space character.
pub fn to_link_reference<'a>(c: impl Into<Cow<'a, [u8]>>) -> Cow<'a, [u8]> {
    let mut c = collapse_spaces(c);
    c = fold_case_full(c);
    if c.first() == Some(&b' ') {
        c = Cow::Owned(c[1..].to_vec());
    }
    if c.last() == Some(&b' ') {
        c = Cow::Owned(c[..c.len() - 1].to_vec());
    }
    c
}

/// Collapses consecutive space characters into a single space character.
pub fn collapse_spaces<'a>(c: impl Into<Cow<'a, [u8]>>) -> Cow<'a, [u8]> {
    let c: Cow<'a, [u8]> = c.into();
    let s: &[u8] = c.as_ref();

    let mut prev_space = false;

    for (i, &b) in s.iter().enumerate() {
        let cur_space = is_space(b);

        if cur_space && (prev_space || b != b' ') {
            let mut out = Vec::with_capacity(s.len());
            out.extend_from_slice(&s[..i]);

            if cur_space {
                if !prev_space {
                    out.push(b' ');
                    prev_space = true;
                }
            } else {
                out.push(b);
                prev_space = false;
            }

            for &b2 in &s[i + 1..] {
                let cur2 = is_space(b2);
                if cur2 {
                    if prev_space {
                        continue;
                    }
                    out.push(b' ');
                    prev_space = true;
                } else {
                    out.push(b2);
                    prev_space = false;
                }
            }

            return Cow::Owned(out);
        }

        prev_space = cur_space;
    }

    c
}

// }}} String utilities

// Unicode utilities {{{

/// Returns the length of a UTF-8 sequence based on the first byte.
#[inline]
pub fn utf8_len(b0: u8) -> Option<usize> {
    if b0 < 0x80 {
        return Some(1);
    }
    if (b0 & 0xC0) == 0x80 {
        return None;
    }
    if (b0 & 0xE0) == 0xC0 {
        return if b0 >= 0xC2 { Some(2) } else { None };
    }
    if (b0 & 0xF0) == 0xE0 {
        return Some(3);
    }
    if (b0 & 0xF8) == 0xF0 && b0 <= 0xF4 {
        return Some(4);
    }
    None
}

/// Returns the character at the given index in the byte slice.
pub fn char_at(line: &[u8], i: usize) -> Option<char> {
    if i >= line.len() {
        return None;
    }
    if let Some(len) = utf8_len(line[i]) {
        if len == 1 {
            return Some(line[i] as char);
        }
        return str::from_utf8(&line[i..i + len])
            .ok()
            .and_then(|s| s.chars().next());
    }
    None
}

unsafe fn to_char_unchecked(bytes: &[u8]) -> char {
    let b0 = *bytes.get_unchecked(0);
    let mut cp: u32 = if b0 < 0xE0 {
        // 110xxxxx
        (b0 & 0x1F) as u32
    } else if b0 < 0xF0 {
        // 1110xxxx
        (b0 & 0x0F) as u32
    } else {
        // 11110xxx
        (b0 & 0x07) as u32
    };

    let mut i = 1;
    while i < bytes.len() {
        let bx = *bytes.get_unchecked(i);
        cp = (cp << 6) | ((bx & 0x3F) as u32);
        i += 1;
    }
    core::char::from_u32_unchecked(cp)
}

/// Folds the case of the given byte slice using full Unicode case folding.
pub fn fold_case_full<'a>(c: impl Into<Cow<'a, [u8]>>) -> Cow<'a, [u8]> {
    let cw = c.into();
    let mut cow = CowByteBuffer::new(cw);
    let len = cow.len();
    while let Some(&b) = cow.next_byte() {
        if b < 0xb5 {
            if (0x41..=0x5a).contains(&b) {
                // A-Z to a-z
                cow.write_byte(b + 32, 0);
            }
            continue;
        }
        if let Some(utf8_len) = utf8_len(b) {
            if cow.pos() + utf8_len <= len {
                let ch = unsafe { to_char_unchecked(&cow[cow.pos()..cow.pos() + utf8_len]) };
                if let Some(folded) = UNICODE_CASE_FOLDINGS.get(&ch) {
                    cow.write_bytes(folded.as_bytes(), utf8_len - 1);
                }
            }
        }
    }
    cow.end()
}

/// Returns true if the given character is a Unicode space, otherwise false.
/// Taken from [cmark](https://github.com/commonmark/cmark).
pub fn is_unicode_space(c: char) -> bool {
    let uc = c as u32;
    uc == 9
        || uc == 10
        || uc == 12
        || uc == 13
        || uc == 32
        || uc == 160
        || uc == 5760
        || (8192..=8202).contains(&uc)
        || uc == 8239
        || uc == 8287
        || uc == 12288
}

/// Returns true if the given character is a Unicode symbol or punctuation, otherwise false.
/// Taken from [cmark](https://github.com/commonmark/cmark).
#[allow(clippy::all)]
pub fn is_unicode_symbol_or_punct(c: char) -> bool {
    let uc = c as u32;
    if uc < 128 {
        is_punct(c as u8)
    } else {
        uc > 128
            && ((uc >= 161 && uc <= 169)
                || (uc >= 171 && uc <= 172)
                || (uc >= 174 && uc <= 177)
                || (uc == 180)
                || (uc >= 182 && uc <= 184)
                || (uc == 187)
                || (uc == 191)
                || (uc == 215)
                || (uc == 247)
                || (uc >= 706 && uc <= 709)
                || (uc >= 722 && uc <= 735)
                || (uc >= 741 && uc <= 747)
                || (uc == 749)
                || (uc >= 751 && uc <= 767)
                || (uc == 885)
                || (uc == 894)
                || (uc >= 900 && uc <= 901)
                || (uc == 903)
                || (uc == 1014)
                || (uc == 1154)
                || (uc >= 1370 && uc <= 1375)
                || (uc >= 1417 && uc <= 1418)
                || (uc >= 1421 && uc <= 1423)
                || (uc == 1470)
                || (uc == 1472)
                || (uc == 1475)
                || (uc == 1478)
                || (uc >= 1523 && uc <= 1524)
                || (uc >= 1542 && uc <= 1551)
                || (uc == 1563)
                || (uc >= 1565 && uc <= 1567)
                || (uc >= 1642 && uc <= 1645)
                || (uc == 1748)
                || (uc == 1758)
                || (uc == 1769)
                || (uc >= 1789 && uc <= 1790)
                || (uc >= 1792 && uc <= 1805)
                || (uc >= 2038 && uc <= 2041)
                || (uc >= 2046 && uc <= 2047)
                || (uc >= 2096 && uc <= 2110)
                || (uc == 2142)
                || (uc == 2184)
                || (uc >= 2404 && uc <= 2405)
                || (uc == 2416)
                || (uc >= 2546 && uc <= 2547)
                || (uc >= 2554 && uc <= 2555)
                || (uc == 2557)
                || (uc == 2678)
                || (uc >= 2800 && uc <= 2801)
                || (uc == 2928)
                || (uc >= 3059 && uc <= 3066)
                || (uc == 3191)
                || (uc == 3199)
                || (uc == 3204)
                || (uc == 3407)
                || (uc == 3449)
                || (uc == 3572)
                || (uc == 3647)
                || (uc == 3663)
                || (uc >= 3674 && uc <= 3675)
                || (uc >= 3841 && uc <= 3863)
                || (uc >= 3866 && uc <= 3871)
                || (uc == 3892)
                || (uc == 3894)
                || (uc == 3896)
                || (uc >= 3898 && uc <= 3901)
                || (uc == 3973)
                || (uc >= 4030 && uc <= 4037)
                || (uc >= 4039 && uc <= 4044)
                || (uc >= 4046 && uc <= 4058)
                || (uc >= 4170 && uc <= 4175)
                || (uc >= 4254 && uc <= 4255)
                || (uc == 4347)
                || (uc >= 4960 && uc <= 4968)
                || (uc >= 5008 && uc <= 5017)
                || (uc == 5120)
                || (uc >= 5741 && uc <= 5742)
                || (uc >= 5787 && uc <= 5788)
                || (uc >= 5867 && uc <= 5869)
                || (uc >= 5941 && uc <= 5942)
                || (uc >= 6100 && uc <= 6102)
                || (uc >= 6104 && uc <= 6107)
                || (uc >= 6144 && uc <= 6154)
                || (uc == 6464)
                || (uc >= 6468 && uc <= 6469)
                || (uc >= 6622 && uc <= 6655)
                || (uc >= 6686 && uc <= 6687)
                || (uc >= 6816 && uc <= 6822)
                || (uc >= 6824 && uc <= 6829)
                || (uc >= 7002 && uc <= 7018)
                || (uc >= 7028 && uc <= 7038)
                || (uc >= 7164 && uc <= 7167)
                || (uc >= 7227 && uc <= 7231)
                || (uc >= 7294 && uc <= 7295)
                || (uc >= 7360 && uc <= 7367)
                || (uc == 7379)
                || (uc == 8125)
                || (uc >= 8127 && uc <= 8129)
                || (uc >= 8141 && uc <= 8143)
                || (uc >= 8157 && uc <= 8159)
                || (uc >= 8173 && uc <= 8175)
                || (uc >= 8189 && uc <= 8190)
                || (uc >= 8208 && uc <= 8231)
                || (uc >= 8240 && uc <= 8286)
                || (uc >= 8314 && uc <= 8318)
                || (uc >= 8330 && uc <= 8334)
                || (uc >= 8352 && uc <= 8384)
                || (uc >= 8448 && uc <= 8449)
                || (uc >= 8451 && uc <= 8454)
                || (uc >= 8456 && uc <= 8457)
                || (uc == 8468)
                || (uc >= 8470 && uc <= 8472)
                || (uc >= 8478 && uc <= 8483)
                || (uc == 8485)
                || (uc == 8487)
                || (uc == 8489)
                || (uc == 8494)
                || (uc >= 8506 && uc <= 8507)
                || (uc >= 8512 && uc <= 8516)
                || (uc >= 8522 && uc <= 8525)
                || (uc == 8527)
                || (uc >= 8586 && uc <= 8587)
                || (uc >= 8592 && uc <= 9254)
                || (uc >= 9280 && uc <= 9290)
                || (uc >= 9372 && uc <= 9449)
                || (uc >= 9472 && uc <= 10101)
                || (uc >= 10132 && uc <= 11123)
                || (uc >= 11126 && uc <= 11157)
                || (uc >= 11159 && uc <= 11263)
                || (uc >= 11493 && uc <= 11498)
                || (uc >= 11513 && uc <= 11516)
                || (uc >= 11518 && uc <= 11519)
                || (uc == 11632)
                || (uc >= 11776 && uc <= 11822)
                || (uc >= 11824 && uc <= 11869)
                || (uc >= 11904 && uc <= 11929)
                || (uc >= 11931 && uc <= 12019)
                || (uc >= 12032 && uc <= 12245)
                || (uc >= 12272 && uc <= 12283)
                || (uc >= 12289 && uc <= 12292)
                || (uc >= 12296 && uc <= 12320)
                || (uc == 12336)
                || (uc >= 12342 && uc <= 12343)
                || (uc >= 12349 && uc <= 12351)
                || (uc >= 12443 && uc <= 12444)
                || (uc == 12448)
                || (uc == 12539)
                || (uc >= 12688 && uc <= 12689)
                || (uc >= 12694 && uc <= 12703)
                || (uc >= 12736 && uc <= 12771)
                || (uc >= 12800 && uc <= 12830)
                || (uc >= 12842 && uc <= 12871)
                || (uc == 12880)
                || (uc >= 12896 && uc <= 12927)
                || (uc >= 12938 && uc <= 12976)
                || (uc >= 12992 && uc <= 13311)
                || (uc >= 19904 && uc <= 19967)
                || (uc >= 42128 && uc <= 42182)
                || (uc >= 42238 && uc <= 42239)
                || (uc >= 42509 && uc <= 42511)
                || (uc == 42611)
                || (uc == 42622)
                || (uc >= 42738 && uc <= 42743)
                || (uc >= 42752 && uc <= 42774)
                || (uc >= 42784 && uc <= 42785)
                || (uc >= 42889 && uc <= 42890)
                || (uc >= 43048 && uc <= 43051)
                || (uc >= 43062 && uc <= 43065)
                || (uc >= 43124 && uc <= 43127)
                || (uc >= 43214 && uc <= 43215)
                || (uc >= 43256 && uc <= 43258)
                || (uc == 43260)
                || (uc >= 43310 && uc <= 43311)
                || (uc == 43359)
                || (uc >= 43457 && uc <= 43469)
                || (uc >= 43486 && uc <= 43487)
                || (uc >= 43612 && uc <= 43615)
                || (uc >= 43639 && uc <= 43641)
                || (uc >= 43742 && uc <= 43743)
                || (uc >= 43760 && uc <= 43761)
                || (uc == 43867)
                || (uc >= 43882 && uc <= 43883)
                || (uc == 44011)
                || (uc == 64297)
                || (uc >= 64434 && uc <= 64450)
                || (uc >= 64830 && uc <= 64847)
                || (uc == 64975)
                || (uc >= 65020 && uc <= 65023)
                || (uc >= 65040 && uc <= 65049)
                || (uc >= 65072 && uc <= 65106)
                || (uc >= 65108 && uc <= 65126)
                || (uc >= 65128 && uc <= 65131)
                || (uc >= 65281 && uc <= 65295)
                || (uc >= 65306 && uc <= 65312)
                || (uc >= 65339 && uc <= 65344)
                || (uc >= 65371 && uc <= 65381)
                || (uc >= 65504 && uc <= 65510)
                || (uc >= 65512 && uc <= 65518)
                || (uc >= 65532 && uc <= 65533)
                || (uc >= 65792 && uc <= 65794)
                || (uc >= 65847 && uc <= 65855)
                || (uc >= 65913 && uc <= 65929)
                || (uc >= 65932 && uc <= 65934)
                || (uc >= 65936 && uc <= 65948)
                || (uc == 65952)
                || (uc >= 66000 && uc <= 66044)
                || (uc == 66463)
                || (uc == 66512)
                || (uc == 66927)
                || (uc == 67671)
                || (uc >= 67703 && uc <= 67704)
                || (uc == 67871)
                || (uc == 67903)
                || (uc >= 68176 && uc <= 68184)
                || (uc == 68223)
                || (uc == 68296)
                || (uc >= 68336 && uc <= 68342)
                || (uc >= 68409 && uc <= 68415)
                || (uc >= 68505 && uc <= 68508)
                || (uc == 69293)
                || (uc >= 69461 && uc <= 69465)
                || (uc >= 69510 && uc <= 69513)
                || (uc >= 69703 && uc <= 69709)
                || (uc >= 69819 && uc <= 69820)
                || (uc >= 69822 && uc <= 69825)
                || (uc >= 69952 && uc <= 69955)
                || (uc >= 70004 && uc <= 70005)
                || (uc >= 70085 && uc <= 70088)
                || (uc == 70093)
                || (uc == 70107)
                || (uc >= 70109 && uc <= 70111)
                || (uc >= 70200 && uc <= 70205)
                || (uc == 70313)
                || (uc >= 70731 && uc <= 70735)
                || (uc >= 70746 && uc <= 70747)
                || (uc == 70749)
                || (uc == 70854)
                || (uc >= 71105 && uc <= 71127)
                || (uc >= 71233 && uc <= 71235)
                || (uc >= 71264 && uc <= 71276)
                || (uc == 71353)
                || (uc >= 71484 && uc <= 71487)
                || (uc == 71739)
                || (uc >= 72004 && uc <= 72006)
                || (uc == 72162)
                || (uc >= 72255 && uc <= 72262)
                || (uc >= 72346 && uc <= 72348)
                || (uc >= 72350 && uc <= 72354)
                || (uc >= 72448 && uc <= 72457)
                || (uc >= 72769 && uc <= 72773)
                || (uc >= 72816 && uc <= 72817)
                || (uc >= 73463 && uc <= 73464)
                || (uc >= 73539 && uc <= 73551)
                || (uc >= 73685 && uc <= 73713)
                || (uc == 73727)
                || (uc >= 74864 && uc <= 74868)
                || (uc >= 77809 && uc <= 77810)
                || (uc >= 92782 && uc <= 92783)
                || (uc == 92917)
                || (uc >= 92983 && uc <= 92991)
                || (uc >= 92996 && uc <= 92997)
                || (uc >= 93847 && uc <= 93850)
                || (uc == 94178)
                || (uc == 113820)
                || (uc == 113823)
                || (uc >= 118608 && uc <= 118723)
                || (uc >= 118784 && uc <= 119029)
                || (uc >= 119040 && uc <= 119078)
                || (uc >= 119081 && uc <= 119140)
                || (uc >= 119146 && uc <= 119148)
                || (uc >= 119171 && uc <= 119172)
                || (uc >= 119180 && uc <= 119209)
                || (uc >= 119214 && uc <= 119274)
                || (uc >= 119296 && uc <= 119361)
                || (uc == 119365)
                || (uc >= 119552 && uc <= 119638)
                || (uc == 120513)
                || (uc == 120539)
                || (uc == 120571)
                || (uc == 120597)
                || (uc == 120629)
                || (uc == 120655)
                || (uc == 120687)
                || (uc == 120713)
                || (uc == 120745)
                || (uc == 120771)
                || (uc >= 120832 && uc <= 121343)
                || (uc >= 121399 && uc <= 121402)
                || (uc >= 121453 && uc <= 121460)
                || (uc >= 121462 && uc <= 121475)
                || (uc >= 121477 && uc <= 121483)
                || (uc == 123215)
                || (uc == 123647)
                || (uc >= 125278 && uc <= 125279)
                || (uc == 126124)
                || (uc == 126128)
                || (uc == 126254)
                || (uc >= 126704 && uc <= 126705)
                || (uc >= 126976 && uc <= 127019)
                || (uc >= 127024 && uc <= 127123)
                || (uc >= 127136 && uc <= 127150)
                || (uc >= 127153 && uc <= 127167)
                || (uc >= 127169 && uc <= 127183)
                || (uc >= 127185 && uc <= 127221)
                || (uc >= 127245 && uc <= 127405)
                || (uc >= 127462 && uc <= 127490)
                || (uc >= 127504 && uc <= 127547)
                || (uc >= 127552 && uc <= 127560)
                || (uc >= 127568 && uc <= 127569)
                || (uc >= 127584 && uc <= 127589)
                || (uc >= 127744 && uc <= 128727)
                || (uc >= 128732 && uc <= 128748)
                || (uc >= 128752 && uc <= 128764)
                || (uc >= 128768 && uc <= 128886)
                || (uc >= 128891 && uc <= 128985)
                || (uc >= 128992 && uc <= 129003)
                || (uc == 129008)
                || (uc >= 129024 && uc <= 129035)
                || (uc >= 129040 && uc <= 129095)
                || (uc >= 129104 && uc <= 129113)
                || (uc >= 129120 && uc <= 129159)
                || (uc >= 129168 && uc <= 129197)
                || (uc >= 129200 && uc <= 129201)
                || (uc >= 129280 && uc <= 129619)
                || (uc >= 129632 && uc <= 129645)
                || (uc >= 129648 && uc <= 129660)
                || (uc >= 129664 && uc <= 129672)
                || (uc >= 129680 && uc <= 129725)
                || (uc >= 129727 && uc <= 129733)
                || (uc >= 129742 && uc <= 129755)
                || (uc >= 129760 && uc <= 129768)
                || (uc >= 129776 && uc <= 129784)
                || (uc >= 129792 && uc <= 129938)
                || (uc >= 129940 && uc <= 129994))
    }
}

// }}} Unicode utilities

// HTML utilities {{{

const HTML_QUOTE: &str = "&quot;";
const HTML_AMP: &str = "&amp;";
const HTML_LESS: &str = "&lt;";
const HTML_GREATER: &str = "&gt;";

const HTML_ESCAPE_TABLE: [Option<&str>; 256] = {
    let mut table = [None; 256];
    table[0] = Some("\u{FFFD}");
    table[b'"' as usize] = Some(HTML_QUOTE);
    table[b'&' as usize] = Some(HTML_AMP);
    table[b'<' as usize] = Some(HTML_LESS);
    table[b'>' as usize] = Some(HTML_GREATER);
    table
};

/// Returns HTML escaped bytes if the given byte should be escaped,
/// otherwise None.
#[inline(always)]
pub fn try_escape_html_byte(b: u8) -> Option<&'static str> {
    HTML_ESCAPE_TABLE[b as usize]
}

/// Escapes HTML special characters in the given byte slice.
pub fn escape_html<'a>(c: impl Into<Cow<'a, [u8]>>) -> Cow<'a, [u8]> {
    let s = c.into();
    let mut result: Option<Vec<u8>> = None;
    let mut last_pos = 0;
    for (i, &b) in s.iter().enumerate() {
        if let Some(escaped) = try_escape_html_byte(b) {
            if result.is_none() {
                result = Some(Vec::with_capacity(s.len() + 16));
            }
            let res = result.as_mut().unwrap();
            res.extend_from_slice(&s[last_pos..i]);
            res.extend_from_slice(escaped.as_bytes());
            last_pos = i + 1;
        }
    }
    if let Some(mut res) = result {
        res.extend_from_slice(&s[last_pos..]);
        Cow::Owned(res)
    } else {
        s
    }
}

/// Tries to resolve numeric references like '&#1234;'.
/// `s[pos]` must be '&' .
/// This function does not check `s[pos]` is '&' .
#[inline(always)]
pub(crate) fn try_resolve_numeric_reference(s: &[u8], pos: usize) -> Option<(usize, char)> {
    let stop = s.len();
    if pos + 2 >= stop || s[pos + 1] != b'#' {
        return None;
    }
    let mut i = pos + 2;
    let (is_hex, ok) = if i < stop && (s[i] == b'x' || s[i] == b'X') {
        i += 1;
        let num_start = i;
        while i < stop && s[i].is_ascii_hexdigit() {
            i += 1;
        }
        (true, i - num_start < 7)
    } else {
        let num_start = i;
        while i < stop && s[i].is_ascii_digit() {
            i += 1;
        }
        (false, i - num_start < 8)
    };
    if ok && i < stop && s[i] == b';' {
        let num_str =
            unsafe { str::from_utf8_unchecked(&s[pos + 2 + if is_hex { 1 } else { 0 }..i]) };
        let code_point = if is_hex {
            u32::from_str_radix(num_str, 16).ok()
        } else {
            num_str.parse::<u32>().ok()
        };
        if let Some(cp) = code_point {
            if let Some(ch) = char::from_u32(cp) {
                return Some((i - pos, ch));
            }
            return Some((i - pos, '\u{FFFD}'));
        }
    }
    None
}

/// Resolves numeric references like '&#1234;' in the given byte slice.
pub fn resolve_numeric_references<'a>(c: impl Into<Cow<'a, [u8]>>) -> Cow<'a, [u8]> {
    let cw = c.into();
    if memchr::memchr(b'&', cw.as_ref()).is_none() {
        return cw;
    }
    let mut cow = CowByteBuffer::new(cw);
    while let Some(&b) = cow.next_byte() {
        if b == b'&' {
            if let Some((nbyte, ch)) = try_resolve_numeric_reference(&cow, cow.pos()) {
                let mut buf = [0u8; 4];
                let ch_bytes = ch.encode_utf8(&mut buf).as_bytes();
                cow.write_bytes(ch_bytes, nbyte);
            }
        }
    }
    cow.end()
}

/// Tries to resolve entity references like '&ouml;' .
///
/// `s[pos]` must be '&' .
/// This function does not check `s[pos]` is '&' .
#[inline(always)]
pub(crate) fn try_resolve_entity_reference(s: &[u8], pos: usize) -> Option<(usize, &'static str)> {
    let mut i = pos + 1;
    let stop = s.len();
    while i < stop && s[i].is_ascii_alphanumeric() {
        i += 1;
    }
    if i < stop && s[i] == b';' {
        let name = unsafe { str::from_utf8_unchecked(&s[pos + 1..i]) };
        if let Some(replacement) = look_up_html5_entity_by_name(name) {
            return Some((i - pos, replacement));
        }
    }
    None
}

/// Resolves entity references like '&ouml;' in the given byte slice.
///
/// If `html-entities` feature is enabled, this function resolves all HTML5 entities
/// (but it will increase the binary size).
/// Otherwise, it only resolves a small set of common entities.
pub fn resolve_entity_references<'a>(c: impl Into<Cow<'a, [u8]>>) -> Cow<'a, [u8]> {
    let cw = c.into();
    if memchr::memchr(b'&', cw.as_ref()).is_none() {
        return cw;
    }
    let mut cow = CowByteBuffer::new(cw);
    while let Some(&b) = cow.next_byte() {
        if b == b'&' {
            if let Some((nbyte, replacement)) = try_resolve_entity_reference(&cow, cow.pos()) {
                cow.write_bytes(replacement.as_bytes(), nbyte);
            }
        }
    }
    cow.end()
}

/// Looks up an HTML5 entity by its name.
///
/// If `html-entities` feature is enabled, this function resolves all HTML5 entities
/// (but it will increase the binary size).
/// Otherwise, it only resolves a small set of common entities.
pub fn look_up_html5_entity_by_name(name: &str) -> Option<&'static str> {
    #[cfg(feature = "html-entities")]
    {
        crate::html_entity::look_up_html5_entity_by_name(name)
    }

    #[cfg(not(feature = "html-entities"))]
    {
        Some(match name {
            "lt" => "<",
            "gt" => ">",
            "amp" => "&",
            "quot" => "\"",
            "apos" => "'",

            "nbsp" => "\u{00A0}",
            "copy" => "©",
            "reg" => "®",
            "trade" => "™",
            "hellip" => "…",
            "ndash" => "–",
            "mdash" => "—",

            "lsquo" => "‘",
            "rsquo" => "’",
            "ldquo" => "“",
            "rdquo" => "”",

            "times" => "×",
            "divide" => "÷",
            "minus" => "−",
            "plusmn" => "±",
            "deg" => "°",

            "yen" => "¥",
            "euro" => "€",
            "pound" => "£",
            "cent" => "¢",

            "larr" => "←",
            "uarr" => "↑",
            "rarr" => "→",
            "darr" => "↓",

            _ => return None,
        })
    }
}

const fn build_urlsafe_table() -> [bool; 256] {
    let mut t = [false; 256];

    let mut b = b'A';
    while b <= b'Z' {
        t[b as usize] = true;
        b += 1;
    }
    b = b'a';
    while b <= b'z' {
        t[b as usize] = true;
        b += 1;
    }
    b = b'0';
    while b <= b'9' {
        t[b as usize] = true;
        b += 1;
    }

    t[b'-' as usize] = true;
    t[b'.' as usize] = true;
    t[b'_' as usize] = true;
    t[b'~' as usize] = true;
    t[b'?' as usize] = true;
    t[b'=' as usize] = true;
    t[b'*' as usize] = true;
    t[b'(' as usize] = true;
    t[b')' as usize] = true;
    t[b'#' as usize] = true;
    t[b'@' as usize] = true;
    t[b'&' as usize] = true;
    t[b'?' as usize] = true;
    t[b'+' as usize] = true;
    t[b',' as usize] = true;
    t[b'\'' as usize] = true;

    t
}

const URL_SAFE_TABLE: [bool; 256] = build_urlsafe_table();

/// Options for URL escaping.
#[derive(Clone, Copy, Default)]
pub struct EscapeUrlOptions {
    /// Whether to resolve references before escaping.
    pub resolves_refs: bool,

    /// Whether to handle escaped spaces.
    pub escaped_space: bool,

    /// Additional characters that should not be escaped.
    pub safe_characters: &'static str,
}

impl EscapeUrlOptions {
    /// Returns the default options for URL escaping.
    /// This does not encode ':' and '/'.
    pub fn for_url() -> Self {
        Self {
            resolves_refs: false,
            escaped_space: false,
            safe_characters: ":/",
        }
    }

    /// Returns the options for escaping URL components.
    pub fn for_component() -> Self {
        Self {
            resolves_refs: false,
            escaped_space: false,
            safe_characters: "",
        }
    }
}

/// Escapes the given byte slice for use in URLs.
pub fn escape_url<'a>(c: impl Into<Cow<'a, [u8]>>, options: &EscapeUrlOptions) -> Cow<'a, [u8]> {
    #[inline]
    fn to_hex(n: u8) -> u8 {
        match n {
            0..=9 => b'0' + n,
            _ => b'A' + (n - 10),
        }
    }
    let resolves_refs = options.resolves_refs;
    let escaped_space = options.escaped_space;

    let mut cw = c.into();
    if resolves_refs {
        cw = unescape_puncts(cw, escaped_space);
        cw = resolve_numeric_references(cw);
        cw = resolve_entity_references(cw);
    }
    let mut cow = CowByteBuffer::new(cw);
    let mut buf: Option<Vec<u8>> = None;
    while let Some(&b) = cow.next_byte() {
        if URL_SAFE_TABLE[b as usize] {
            continue;
        }
        if options.safe_characters.as_bytes().contains(&b) {
            continue;
        }

        // skip already encoded bytes
        if b == b'%'
            && cow.pos() + 2 < cow.len()
            && cow.peek_byte(1).unwrap().is_ascii_hexdigit()
            && cow.peek_byte(2).unwrap().is_ascii_hexdigit()
        {
            cow.skip(2);
            continue;
        }

        // skip invalid UTF-8 bytes
        let Some(u8len) = utf8_len(b) else {
            continue;
        };

        if b == b' ' {
            cow.write_bytes(b"%20", 0);
            continue;
        }

        if buf.is_none() {
            // 3: '%XX'
            // 4: for max 4 bytes in UTF-8
            buf = Some(Vec::with_capacity(3 * 4));
        }
        let buf = buf.as_mut().unwrap();
        buf.clear();
        for i in cow.pos()..min(cow.pos() + u8len, cow.len()) {
            let b = cow[i];
            buf.push(b'%');
            buf.push(to_hex((b >> 4) & 0xF));
            buf.push(to_hex(b & 0xF));
        }
        cow.write_bytes(buf.as_slice(), u8len - 1);
    }
    cow.end()
}

// }}} HTML utilities

// Prioritized {{{

/// An item with a priority.
#[derive(Debug, Clone)]
pub struct Prioritized<T> {
    item: Option<T>,
    priority: u32,
}

impl<T> Prioritized<T> {
    pub fn new(item: T, priority: u32) -> Self {
        Self {
            item: Some(item),
            priority,
        }
    }

    /// Returns a reference to the item.
    #[inline(always)]
    pub fn item(&self) -> &T {
        self.item.as_ref().unwrap()
    }

    /// Returns a mutable reference to the item.
    #[inline(always)]
    pub fn item_mut(&mut self) -> &mut T {
        self.item.as_mut().unwrap()
    }

    /// Takes the item, leaving an uninitialized value in its place.
    pub fn take(&mut self) -> T {
        self.item.take().unwrap()
    }

    /// Returns the priority.
    #[inline(always)]
    pub fn priority(&self) -> u32 {
        self.priority
    }
}

impl<T> PartialEq for Prioritized<T> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}
impl<T> Eq for Prioritized<T> {}

impl<T> PartialOrd for Prioritized<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Prioritized<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority.cmp(&other.priority)
    }
}

// }}} Prioritized

// StringMap {{{

/// A simple map from strings to values.
///
/// StringMap keeps the entries in insertion order.
/// This is not optimized for performance, but it is simple and works well for small maps.
/// e.g. for attributes of an HTML element, which usually has only a few attributes.
#[derive(Default, Clone)]
pub struct StringMap<V> {
    entries: Vec<(String, V)>,
}

impl<V> StringMap<V> {
    /// Creates a new empty [`StringMap`].
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Creates a new [`StringMap`] with the specified capacity.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
        }
    }

    /// Returns the number of entries in the map.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Removes all entries from the map.
    pub fn clear(&mut self) {
        self.entries.clear()
    }

    fn position<Q>(&self, key: &Q) -> Option<usize>
    where
        String: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.entries.iter().position(|(k, _)| k.borrow() == key)
    }

    /// Inserts a key-value pair into the map.
    ///
    /// If the map did not have this key present, None is returned.
    /// If the map did have this key present, the value is updated, and the old value is returned.
    pub fn insert(&mut self, key: impl Into<String>, value: V) -> Option<V> {
        let key = key.into();
        if let Some(i) = self.entries.iter().position(|(k, _)| k == &key) {
            let old = core::mem::replace(&mut self.entries[i].1, value);
            return Some(old);
        }
        self.entries.push((key, value));
        None
    }

    /// Returns a reference to the value corresponding to the key.
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        String: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.position(key).map(|i| &self.entries[i].1)
    }

    /// Returns a mutable reference to the value corresponding to the key.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        String: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.position(key).map(|i| &mut self.entries[i].1)
    }

    /// Removes a key from the map, returning the value at the key if the key was previously in the
    /// map.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        String: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        let i = self.position(key)?;
        Some(self.entries.remove(i).1) // 挿入順維持
    }

    /// Returns an iterator over the key-value pairs in the map, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Returns an iterator over the key-value pairs in the map, in insertion order, with mutable
    /// references to the values.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut V)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    /// Returns an iterator over the keys in the map, in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// Returns an iterator over the values in the map, in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }

    /// Returns an iterator over the values in the map, in insertion order, with mutable references
    /// to the values.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    /// Returns true if the map contains a value for the specified key.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        String: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        self.position(key).is_some()
    }
}

impl<V> Index<&str> for StringMap<V> {
    type Output = V;
    fn index(&self, key: &str) -> &Self::Output {
        self.get(key).expect("key not found")
    }
}

impl<V: Debug> Debug for StringMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut m = f.debug_map();
        for (k, v) in &self.entries {
            m.entry(k, v);
        }
        m.finish()
    }
}

impl<V: PartialEq> PartialEq for StringMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}
impl<V: Eq> Eq for StringMap<V> {}

impl<V> Extend<(String, V)> for StringMap<V> {
    fn extend<T: IntoIterator<Item = (String, V)>>(&mut self, iter: T) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

impl<'a, V> Extend<(&'a str, V)> for StringMap<V> {
    fn extend<T: IntoIterator<Item = (&'a str, V)>>(&mut self, iter: T) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

impl<V> FromIterator<(String, V)> for StringMap<V> {
    fn from_iter<T: IntoIterator<Item = (String, V)>>(iter: T) -> Self {
        let iter = iter.into_iter();
        let (lower, _) = iter.size_hint();
        let mut m = Self::with_capacity(lower);
        m.extend(iter);
        m
    }
}

impl<'a, V> FromIterator<(&'a str, V)> for StringMap<V> {
    fn from_iter<T: IntoIterator<Item = (&'a str, V)>>(iter: T) -> Self {
        let iter = iter.into_iter();
        let (lower, _) = iter.size_hint();
        let mut m = Self::with_capacity(lower);
        for (k, v) in iter {
            m.insert(k, v);
        }
        m
    }
}

pub struct StringMapIntoIter<V>(<Vec<(String, V)> as IntoIterator>::IntoIter);

impl<V> Iterator for StringMapIntoIter<V> {
    type Item = (String, V);
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl<V> ExactSizeIterator for StringMapIntoIter<V> {}

impl<V> IntoIterator for StringMap<V> {
    type Item = (String, V);
    type IntoIter = StringMapIntoIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        StringMapIntoIter(self.entries.into_iter())
    }
}

impl<'a, V> IntoIterator for &'a StringMap<V> {
    type Item = (&'a String, &'a V);
    type IntoIter =
        core::iter::Map<core::slice::Iter<'a, (String, V)>, fn(&(String, V)) -> (&String, &V)>;

    fn into_iter(self) -> Self::IntoIter {
        fn to_refs<V>((k, v): &(String, V)) -> (&String, &V) {
            (k, v)
        }
        self.entries.iter().map(to_refs::<V>)
    }
}

impl<'a, V> IntoIterator for &'a mut StringMap<V> {
    type Item = (&'a String, &'a mut V);
    type IntoIter = core::iter::Map<
        core::slice::IterMut<'a, (String, V)>,
        fn(&mut (String, V)) -> (&String, &mut V),
    >;

    fn into_iter(self) -> Self::IntoIter {
        fn to_refs_mut<V>((k, v): &mut (String, V)) -> (&String, &mut V) {
            (&*k, v)
        }
        self.entries.iter_mut().map(to_refs_mut::<V>)
    }
}

// }}}

// TinyVec {{{

#[derive(Debug, Clone, PartialEq, Eq)]
enum TinyVecKind<T> {
    Empty,
    Single(T),
    Collection(Vec<T>),
}

/// A vector-like collection that can hold either a single item or a collection of items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TinyVec<T> {
    kind: TinyVecKind<T>,
}

impl<T> TinyVec<T> {
    /// Creates a new TinyVec containing the given values.
    pub fn new(values: Vec<T>) -> Self {
        if values.len() == 1 {
            Self::from_single(values.into_iter().next().unwrap())
        } else {
            Self::from_vec(values)
        }
    }

    /// Creates a new empty TinyVec.
    pub fn empty() -> Self {
        Self {
            kind: TinyVecKind::Empty,
        }
    }

    /// Creates a new TinyVec containing a single item.
    pub const fn from_single(value: T) -> Self {
        Self {
            kind: TinyVecKind::Single(value),
        }
    }

    /// Creates a new TinyVec containing a collection of items.
    pub fn from_vec(values: Vec<T>) -> Self {
        Self {
            kind: TinyVecKind::Collection(values),
        }
    }

    /// Returns the number of items in the TinyVec.
    pub fn len(&self) -> usize {
        match &self.kind {
            TinyVecKind::Empty => 0,
            TinyVecKind::Single(_) => 1,
            TinyVecKind::Collection(v) => v.len(),
        }
    }

    /// Returns true if the TinyVec contains no items.
    pub fn is_empty(&self) -> bool {
        match &self.kind {
            TinyVecKind::Empty => true,
            TinyVecKind::Single(_) => false,
            TinyVecKind::Collection(v) => v.is_empty(),
        }
    }

    /// Returns a slice of the items in the TinyVec.
    pub fn as_slice(&self) -> &[T] {
        match &self.kind {
            TinyVecKind::Empty => &[],
            TinyVecKind::Single(v) => core::slice::from_ref(v),
            TinyVecKind::Collection(vs) => vs.as_slice(),
        }
    }

    /// Returns a mutable slice of the items in the TinyVec.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        match &mut self.kind {
            TinyVecKind::Empty => &mut [],
            TinyVecKind::Single(v) => core::slice::from_mut(v),
            TinyVecKind::Collection(vs) => vs.as_mut_slice(),
        }
    }

    /// Returns a reference to the item at the specified index, or None if the index is out of
    /// bounds.
    pub fn get(&self, index: usize) -> Option<&T> {
        match &self.kind {
            TinyVecKind::Empty => None,
            TinyVecKind::Single(v) => {
                if index == 0 {
                    Some(v)
                } else {
                    None
                }
            }
            TinyVecKind::Collection(vs) => vs.get(index),
        }
    }

    /// Returns a mutable reference to the item at the specified index, or None if the index is out
    /// of bounds.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        match &mut self.kind {
            TinyVecKind::Empty => None,
            TinyVecKind::Single(v) => {
                if index == 0 {
                    Some(v)
                } else {
                    None
                }
            }
            TinyVecKind::Collection(vs) => vs.get_mut(index),
        }
    }

    /// Appends an item to the end of the TinyVec.
    pub fn push(&mut self, value: T) {
        match &mut self.kind {
            TinyVecKind::Empty => {
                self.kind = TinyVecKind::Single(value);
            }
            TinyVecKind::Single(_) => {
                let old = core::mem::replace(&mut self.kind, TinyVecKind::Collection(Vec::new()));
                match old {
                    TinyVecKind::Empty => unreachable!(),
                    TinyVecKind::Single(v0) => {
                        self.kind = TinyVecKind::Collection(alloc::vec![v0, value]);
                    }
                    TinyVecKind::Collection(_) => unreachable!(),
                }
            }
            TinyVecKind::Collection(vs) => vs.push(value),
        }
    }

    /// Removes and returns the item at the specified index, shifting all items after it to the
    /// left.
    pub fn remove(&mut self, index: usize) -> T {
        match &mut self.kind {
            TinyVecKind::Empty => panic!("index out of bounds"),
            TinyVecKind::Single(_) => {
                if index == 0 {
                    let old =
                        core::mem::replace(&mut self.kind, TinyVecKind::Collection(Vec::new()));
                    match old {
                        TinyVecKind::Empty => unreachable!(),
                        TinyVecKind::Single(v) => v,
                        TinyVecKind::Collection(_) => unreachable!(),
                    }
                } else {
                    panic!("index out of bounds");
                }
            }
            TinyVecKind::Collection(vs) => vs.remove(index),
        }
    }

    /// Converts the TinyVec into a `Vec<T>`.
    pub fn into_vec(self) -> Vec<T> {
        match self.kind {
            TinyVecKind::Empty => Vec::new(),
            TinyVecKind::Single(v) => alloc::vec![v],
            TinyVecKind::Collection(vs) => vs,
        }
    }
}

impl<T> Default for TinyVec<T> {
    fn default() -> Self {
        Self {
            kind: TinyVecKind::Empty,
        }
    }
}

impl<T> Deref for TinyVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for TinyVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T> AsRef<[T]> for TinyVec<T> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T> AsMut<[T]> for TinyVec<T> {
    fn as_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T> From<T> for TinyVec<T> {
    fn from(value: T) -> Self {
        Self::from_single(value)
    }
}

impl<T> From<Vec<T>> for TinyVec<T> {
    fn from(values: Vec<T>) -> Self {
        Self::from_vec(values)
    }
}

impl<T> FromIterator<T> for TinyVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut it = iter.into_iter();
        match it.next() {
            None => TinyVec::from_vec(Vec::new()),
            Some(first) => match it.next() {
                None => TinyVec::from_single(first),
                Some(second) => {
                    let mut v = Vec::new();
                    v.push(first);
                    v.push(second);
                    v.extend(it);
                    TinyVec::from_vec(v)
                }
            },
        }
    }
}

impl<T> Extend<T> for TinyVec<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for x in iter {
            self.push(x);
        }
    }
}

impl<T> Index<usize> for TinyVec<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        match &self.kind {
            TinyVecKind::Empty => panic!("index out of bounds"),
            TinyVecKind::Single(v) => {
                if index == 0 {
                    v
                } else {
                    panic!("index out of bounds");
                }
            }
            TinyVecKind::Collection(vs) => &vs[index],
        }
    }
}

impl<T> IndexMut<usize> for TinyVec<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match &mut self.kind {
            TinyVecKind::Empty => panic!("index out of bounds"),
            TinyVecKind::Single(v) => {
                if index == 0 {
                    v
                } else {
                    panic!("index out of bounds");
                }
            }
            TinyVecKind::Collection(vs) => &mut vs[index],
        }
    }
}

impl<T> IntoIterator for TinyVec<T> {
    type Item = T;
    type IntoIter = alloc::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a, T> IntoIterator for &'a TinyVec<T> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T> IntoIterator for &'a mut TinyVec<T> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}
// }}} TinyVec

// Tests {{{

#[cfg(test)]
mod tests {

    use super::*;
    use alloc::string::ToString;

    #[allow(unused_imports)]
    #[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
    use crate::println;

    #[test]
    fn test_cow_byte_buffer() {
        let source = b"Hello, world!";
        let mut cow = CowByteBuffer::new(source);
        while let Some(b) = cow.next_byte() {
            if *b == b'!' {
                cow.write_byte(b'$', 0);
            }
        }
        let result = cow.end();
        assert_eq!(result.as_ref(), b"Hello, world$");
    }

    #[test]
    fn test_trim_left() {
        let source = b"   Hello, world!   ";
        let trimmed = trim_left(source, b" ");
        assert_eq!(trimmed, b"Hello, world!   ");
    }

    #[test]
    fn test_trim_right() {
        let source = b"   Hello, world!   ";
        let trimmed = trim_right(source, b" ");
        assert_eq!(trimmed, b"   Hello, world!");
    }

    #[test]
    fn test_trim_left_space() {
        let source = b"   Hello, world!   ";
        let trimmed = trim_left_space(source);
        assert_eq!(trimmed, b"Hello, world!   ");
    }

    #[test]
    fn test_trim_right_space() {
        let source = b"   Hello, world!   ";
        let trimmed = trim_right_space(source);
        assert_eq!(trimmed, b"   Hello, world!");
    }

    #[test]
    fn test_is_space() {
        assert!(is_space(b' '));
        assert!(is_space(b'\t'));
        assert!(is_space(b'\n'));
        assert!(!is_space(b'H'));
        assert!(!is_space(b'1'));
    }

    #[test]
    fn test_trim_left_length() {
        let source = b"   Hello, world!   ";
        let length = trim_left_length(source, b" ");
        assert_eq!(length, 3);
    }

    #[test]
    fn test_trim_right_length() {
        let source = b"   Hello, world!   ";
        let length = trim_right_length(source, b" ");
        assert_eq!(length, 3);
    }

    #[test]
    fn test_trim_left_space_length() {
        let source = b"   Hello, world!   ";
        let length = trim_left_space_length(source);
        assert_eq!(length, 3);
    }

    #[test]
    fn test_trim_right_space_length() {
        let source = b"   Hello, world!   ";
        let length = trim_right_space_length(source);
        assert_eq!(length, 3);
    }

    #[test]
    fn test_prioritized() {
        let item1 = Prioritized::new("item1", 10);
        let item2 = Prioritized::new("item2", 20);
        assert!(item1 < item2);
        assert_eq!(item1.item(), &"item1");
        assert_eq!(item2.priority(), 20);

        let item3 = Prioritized::new("item3", 40);

        let mut vector = [item2, item3, item1];
        vector.sort();
        assert_eq!(vector[0].item(), &"item1");
        assert_eq!(vector[1].item(), &"item2");
        assert_eq!(vector[2].item(), &"item3");
    }

    #[test]
    fn test_unescape_puncts() {
        let source = b"Hello\\, world\\! This is a test\\.";
        let unescaped = unescape_puncts(source, false);
        assert_eq!(unescaped.as_ref(), b"Hello, world! This is a test.");
        assert!(matches!(unescaped, Cow::Owned(_)));

        let source2 = b"Escape\\  space\\  here.";
        let unescaped2 = unescape_puncts(source2, true);
        assert_eq!(unescaped2.as_ref(), b"Escape space here.");
        assert!(matches!(unescaped2, Cow::Owned(_)));

        let source3 = b"No escapes here.";
        let unescaped3 = unescape_puncts(source3, false);
        assert_eq!(unescaped3.as_ref(), b"No escapes here.");
        assert!(matches!(unescaped3, Cow::Borrowed(_)));
    }

    #[test]
    fn test_fold_case_full() {
        let source = "Hello Σ World!".as_bytes();
        let folded = fold_case_full(source);
        assert_eq!(folded.as_ref(), "hello σ world!".as_bytes());
        assert!(matches!(folded, Cow::Owned(_)));

        let source2 = "no changes".as_bytes();
        let folded2 = fold_case_full(source2);
        assert_eq!(folded2.as_ref(), "no changes".as_bytes());
        assert!(matches!(folded2, Cow::Borrowed(_)));
    }

    #[test]
    fn test_resolve_numeric_references() {
        let source = b"Hello &#65;&#x42;!";
        let resolved = resolve_numeric_references(source);
        assert_eq!(resolved.as_ref(), b"Hello AB!");
        assert!(matches!(resolved, Cow::Owned(_)));

        let source2 = b"No references here.";
        let resolved2 = resolve_numeric_references(source2);
        assert_eq!(resolved2.as_ref(), b"No references here.");
        assert!(matches!(resolved2, Cow::Borrowed(_)));

        let source3 = b"Hello &#xZZZ;!";
        let resolved3 = resolve_numeric_references(source3);
        assert_eq!(resolved3.as_ref(), b"Hello &#xZZZ;!");
        assert!(matches!(resolved3, Cow::Borrowed(_)));

        let source4 = b"Hello &#;!";
        let resolved4 = resolve_numeric_references(source4);
        assert_eq!(resolved4.as_ref(), b"Hello &#;!");
        assert!(matches!(resolved4, Cow::Borrowed(_)));

        let source5 = b"Hello &#123456789;!";
        let resolved5 = resolve_numeric_references(source5);
        assert_eq!(resolved5.as_ref(), "Hello &#123456789;!".as_bytes());
        assert!(matches!(resolved4, Cow::Borrowed(_)));
    }

    #[test]
    fn test_resolve_entity_references() {
        let source = b"Hello &lt;world&gt; &amp; everyone!";
        let resolved = resolve_entity_references(source);
        assert_eq!(resolved.as_ref(), b"Hello <world> & everyone!");
        assert!(matches!(resolved, Cow::Owned(_)));

        let source2 = b"No entities here.";
        let resolved2 = resolve_entity_references(source2);
        assert_eq!(resolved2.as_ref(), b"No entities here.");
        assert!(matches!(resolved2, Cow::Borrowed(_)));

        let source3 = b"Hello &unknown;!";
        let resolved3 = resolve_entity_references(source3);
        assert_eq!(resolved3.as_ref(), b"Hello &unknown;!");
        assert!(matches!(resolved3, Cow::Borrowed(_)));
    }

    #[test]
    fn test_string_map() {
        let mut map = StringMap::new();
        map.insert("key1".to_string(), 1);
        map.insert("key2".to_string(), 2);
        assert_eq!(map.get("key1"), Some(&1));
        assert_eq!(map.get("key2"), Some(&2));
        assert_eq!(map.get("key3"), None);

        map.insert("key1".to_string(), 10);
        assert_eq!(map.get("key1"), Some(&10));

        let keys: Vec<&String> = map.keys().collect();
        assert_eq!(keys, vec!["key1", "key2"]);

        let values: Vec<&i32> = map.values().collect();
        assert_eq!(values, vec![&10, &2]);

        let removed = map.remove("key1");
        assert_eq!(removed, Some(10));
        assert_eq!(map.get("key1"), None);
        assert_eq!(map.get("key2"), Some(&2));
    }
}

// }}} Tests
