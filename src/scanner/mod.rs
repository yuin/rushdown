//! Scanners for various patterns in text.

mod scanner_gen;

pub(crate) use self::scanner_gen::*;
use crate::{
    text::{self, Reader, Segment, EOS},
    util::is_space,
};

/// Maximum length of an email address: 64 bytes of local part, `@`, and a
/// domain of at most 255 bytes. [`scan_email`] bounds its scan by this length
/// so that long runs of email-like text cannot be parsed in quadratic time.
pub(crate) const EMAIL_SCAN_MAX: usize = 64 + 1 + 255;

/// Trait for scanning input for specific patterns.
pub trait Scan {
    /// Scans the input and returns the position of the pattern if found.
    fn scan(&self, input: &[u8]) -> Option<usize>;
}

impl<F> Scan for F
where
    F: Fn(&[u8]) -> Option<usize>,
{
    fn scan(&self, input: &[u8]) -> Option<usize> {
        self(input)
    }
}

// re2c stuff {{{

const BUFSIZE: usize = 4096;

struct State<'a, T: Reader<'a>> {
    reader: &'a mut T,
    yyinput: [u8; BUFSIZE],
    yylimit: usize,
    yycursor: usize,
    yymarker: usize,
    token: usize,
    eof: bool,
}

impl<'a, T: Reader<'a>> State<'a, T> {
    fn new(reader: &'a mut T) -> Self {
        let yylimit = BUFSIZE - 1;
        State {
            reader,
            yyinput: [255; BUFSIZE],
            yylimit,
            yycursor: yylimit,
            yymarker: yylimit,
            token: yylimit,
            eof: false,
        }
    }
}

impl<'a, T: Reader<'a>> State<'a, T> {
    fn set_position(&mut self, line: usize, pos: Segment) {
        self.reader.set_position(line, pos);
    }
}

#[derive(PartialEq)]
enum Fill {
    Ok,
    Eof,
    LongLexeme,
}

fn fill<'a, T: Reader<'a>>(st: &mut State<'a, T>) -> Fill {
    if st.eof {
        return Fill::Eof;
    }
    if st.token < 1 {
        return Fill::LongLexeme;
    }

    st.yyinput.copy_within(st.token..st.yylimit, 0);
    st.yylimit -= st.token;
    st.yycursor -= st.token;
    st.yymarker = st.yymarker.overflowing_sub(st.token).0;
    st.token = 255;

    let Some((line, _)) = st.reader.peek_line_bytes() else {
        panic!("should not happen")
    };
    let bufsize = BUFSIZE - 1 - st.yylimit;
    if line.len() < bufsize {
        st.reader.advance_line();
        st.yyinput[st.yylimit..st.yylimit + line.len()].copy_from_slice(&line);
        st.yylimit += line.len();
    } else {
        st.reader.advance(bufsize);
        st.yyinput[st.yylimit..st.yylimit + bufsize].copy_from_slice(&line[..bufsize]);
        st.yylimit += bufsize;
    }
    st.eof = st.reader.peek_byte() == 255;
    st.yyinput[st.yylimit] = 255;
    Fill::Ok
}

// }}} re2c stuff

// private utilities for scanners {{{

fn starts_with(s: &[u8], pos: usize, prefix: &[&str], allow_eol: bool) -> Option<usize> {
    if pos >= s.len() {
        if allow_eol {
            return Some(0);
        }
        return None;
    }
    let s2 = &s[pos..];
    for p in prefix {
        let p_bytes = p.as_bytes();
        if s2.starts_with(p_bytes) {
            return Some(p_bytes.len());
        }
    }
    None
}

fn skip_while(s: &[u8], pos: usize, pred: impl Fn(u8) -> bool, allow_eol: bool) -> Option<usize> {
    if pos >= s.len() {
        if allow_eol {
            return Some(0);
        }
        return None;
    }
    let i = pos + s[pos..].iter().take_while(|&&c| pred(c)).count();
    if i > pos {
        return Some(i - pos);
    }
    None
}

fn contains_any_i(s: &[u8], needles: &[&str]) -> bool {
    for n in needles {
        let n_bytes = n.as_bytes();
        if s.windows(n_bytes.len())
            .any(|window| window.eq_ignore_ascii_case(n_bytes))
        {
            return true;
        }
    }
    false
}

fn not_indented(line: &[u8]) -> Option<usize> {
    let i = line.iter().take_while(|&&c| c == b' ').count();
    if i > 3 {
        return None;
    }
    Some(i)
}

fn skip_spaces(line: &[u8], pos: usize) -> usize {
    let mut i = pos;
    while i < line.len() && is_space(line[i]) {
        i += 1;
    }
    i - pos
}

// }}}

// HTML scanners {{{

include!(concat!(env!("OUT_DIR"), "/allowed_block_tags.rs"));

pub(crate) fn scan_html_block_open_1(line: &[u8]) -> Option<()> {
    let mut i = not_indented(line)?;
    i += starts_with(line, i, &["<textarea", "<script", "<pre", "<style"], false)?;
    starts_with(line, i, &["\r\n", " ", "\t", ">", "\n"], true)?;
    Some(())
}

pub(crate) fn scan_html_block_close_1(line: &[u8]) -> Option<()> {
    contains_any_i(line, &["</script>", "</pre>", "</style>", "</textarea>"]).then_some(())
}

pub(crate) fn scan_html_block_open_2(line: &[u8]) -> Option<()> {
    let i = not_indented(line)?;
    starts_with(line, i, &["<!--"], false)?;
    Some(())
}

pub(crate) fn scan_html_block_close_2(line: &[u8]) -> Option<()> {
    contains_any_i(line, &["-->"]).then_some(())
}

pub(crate) fn scan_html_block_open_3(line: &[u8]) -> Option<()> {
    let i = not_indented(line)?;
    starts_with(line, i, &["<?"], false)?;
    Some(())
}

pub(crate) fn scan_html_block_close_3(line: &[u8]) -> Option<()> {
    contains_any_i(line, &["?>"]).then_some(())
}

pub(crate) fn scan_html_block_open_4(line: &[u8]) -> Option<()> {
    let mut i = not_indented(line)?;
    i += starts_with(line, i, &["<!"], false)?;
    skip_while(line, i, |c| c.is_ascii_alphabetic(), false)?;
    Some(())
}

pub(crate) fn scan_html_block_close_4(line: &[u8]) -> Option<()> {
    contains_any_i(line, &[">"]).then_some(())
}

pub(crate) fn scan_html_block_open_5(line: &[u8]) -> Option<()> {
    let i = not_indented(line)?;
    starts_with(line, i, &["<![CDATA["], false)?;
    Some(())
}

pub(crate) fn scan_html_block_close_5(line: &[u8]) -> Option<()> {
    contains_any_i(line, &["]]>"]).then_some(())
}

pub(crate) fn scan_html_block_open_6(line: &[u8]) -> Option<()> {
    let mut i = not_indented(line)?;
    i += starts_with(line, i, &["</", "<"], false)?;
    let tag_name_start = i;
    i += skip_while(line, i, |c| c.is_ascii_alphabetic(), false)?;
    let tag_name =
        unsafe { core::str::from_utf8_unchecked(&line[tag_name_start..i]).to_ascii_lowercase() };
    if !ALLOWED_BLOCK_TAGS.contains_key(&tag_name) {
        return None;
    }
    starts_with(line, i, &["/>", "\r\n", " ", "\t", ">", "\n"], true)?;
    Some(())
}

pub(crate) fn scan_html_block_open_7(line: &[u8]) -> Option<()> {
    let mut i = not_indented(line)?;
    let n = starts_with(line, i, &["</", "<"], false)?;
    let closing = n == 2;
    i += n;
    let tag_name_start = i;
    i += skip_while(line, i, |c| c.is_ascii_alphabetic(), false)?;
    let tag_name =
        unsafe { core::str::from_utf8_unchecked(&line[tag_name_start..i]).to_ascii_lowercase() };
    if ALLOWED_BLOCK_TAGS.contains_key(&tag_name)
        || matches!(tag_name.as_str(), "pre" | "script" | "style" | "textarea")
    {
        return None;
    }

    if let Some(m) = skip_while(line, i, |c| c.is_ascii_alphanumeric() || c == b'-', false) {
        i += m;
    }
    if !closing {
        if let Some(m) = scan_html_attributes(&line[i..]) {
            i += m;
        }
    }
    i += skip_spaces(line, i);
    i += if closing {
        starts_with(line, i, &[">"], false)?
    } else {
        starts_with(line, i, &["/>", ">"], false)?
    };
    i += skip_spaces(line, i);
    (i == line.len()).then_some(())
}

pub(crate) fn scan_html_attributes(s: &[u8]) -> Option<usize> {
    let mut r = text::BasicReader::new(unsafe { core::str::from_utf8_unchecked(s) });
    if scan_html_attributes_reader(&mut r).is_some() {
        Some(r.position().1.start())
    } else {
        None
    }
}

pub(crate) fn scan_html_attributes_reader<'a>(reader: &mut impl text::Reader<'a>) -> Option<()> {
    let (sline, spos) = reader.position();
    let mut has_space = false;
    loop {
        if !has_space && !is_space(reader.peek_byte()) {
            // no more attributes
            return Some(());
        }
        reader.skip_spaces();

        let c = reader.peek_byte();
        if c == EOS || !(c.is_ascii_alphabetic() || c == b'_' || c == b':') {
            return Some(());
        }
        reader.advance(1);
        reader.skip_while(|c| {
            c.is_ascii_alphanumeric() || c == b':' || c == b'.' || c == b'_' || c == b'-'
        });
        has_space = reader.skip_spaces() > 0;
        if reader.peek_byte() == b'=' {
            reader.advance(1);
            reader.skip_spaces();
            let open = reader.peek_byte();
            if open == b'"' || open == b'\'' {
                reader.advance(1);
                reader.skip_while(|c| c != open && c != EOS);
                if reader.peek_byte() == open {
                    reader.advance(1);
                } else {
                    // no closing quote
                    reader.set_position(sline, spos);
                    return None;
                }
            } else if unquoted_html_attr_value_char(open) {
                reader.advance(1);
                reader.skip_while(unquoted_html_attr_value_char);
            } else {
                // invalid value start
                reader.set_position(sline, spos);
                return None;
            }
            continue;
        }
        // name only attribute
    }
}

#[inline]
pub(crate) fn unquoted_html_attr_value_char(c: u8) -> bool {
    !(c <= 0x20 || c == b'"' || c == b'\'' || c == b'=' || c == b'<' || c == b'>' || c == b'`')
}

pub(crate) fn scan_html_tag_name_reader<'a>(reader: &mut impl text::Reader<'a>) -> Option<()> {
    let c = reader.peek_byte();
    if !c.is_ascii_alphabetic() {
        return None;
    }
    reader.advance(1);
    reader.skip_while(|c| c.is_ascii_alphanumeric() || c == b'-');
    Some(())
}

pub(crate) fn scan_html_tag_reader<'a>(reader: &mut impl text::Reader<'a>) -> Option<()> {
    let c = reader.peek_byte();
    if c != b'<' {
        return None;
    }
    let (line, pos) = reader.position();
    let mut f = || {
        reader.advance(1);
        let c = reader.peek_byte();
        let closing = c == b'/';
        if closing {
            reader.advance(1);
        }
        scan_html_tag_name_reader(reader)?;
        if !closing {
            scan_html_attributes_reader(reader)?;
        }
        reader.skip_spaces();
        let c = reader.peek_byte();
        if !closing && c == b'/' {
            reader.advance(1);
            let c = reader.peek_byte();
            if c != b'>' {
                return None;
            }
        } else if c != b'>' {
            return None;
        }
        reader.advance(1);
        Some(())
    };
    f().or_else(|| {
        reader.set_position(line, pos);
        None
    })
}

// }}} HTML_block scanners

// URL scanners {{{

pub(crate) fn scan_protocol_url(buffer: &[u8]) -> Option<usize> {
    let i = memchr::memchr(b':', buffer)?;
    if !buffer[i..].starts_with(b"://") || scan_domain(&buffer[i + 3..]).is_none() {
        return None;
    }
    scan_url(buffer).map(|stop| scan_url_tail(buffer, stop))
}

pub(crate) fn scan_www_url(buffer: &[u8]) -> Option<usize> {
    let n = scan_domain(buffer)?;
    if n <= 4 {
        return None;
    }
    Some(scan_url_tail(buffer, n))
}

fn scan_url_tail(buffer: &[u8], stop: usize) -> usize {
    let mut stop = stop;
    if stop > 0 {
        while stop < buffer.len() && !is_space(buffer[stop]) && buffer[stop] != b'<' {
            stop += 1;
        }
        if let Some(i) = memchr::memchr(b'\\', &buffer[..stop]) {
            stop = i;
        }
    }
    stop
}

/// Scans a domain name, like `example.com`; a bare host like `localhost` is also a
/// domain .
pub(crate) fn scan_domain(buffer: &[u8]) -> Option<usize> {
    let mut n = scan_domain_label(buffer)?;
    while n < buffer.len() && buffer[n] == b'.' {
        match scan_domain_label(&buffer[n + 1..]) {
            Some(m) => {
                n += 1 + m;
                if n > 255 {
                    return None;
                }
            }
            None => break,
        }
    }
    Some(n)
}

fn scan_domain_label(buffer: &[u8]) -> Option<usize> {
    if !buffer.first()?.is_ascii_alphanumeric() {
        return None;
    }
    let limit = buffer.len().min(63);
    let mut i = 1;
    while i < limit && (buffer[i].is_ascii_alphanumeric() || buffer[i] == b'-') {
        i += 1;
    }
    while i > 1 && buffer[i - 1] == b'-' {
        i -= 1;
    }
    Some(i)
}

// }}}

// Other scanners {{{
pub(crate) fn scan_task_list_item(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    i += starts_with(buffer, i, &["[X]", "[x]", "[ ]"], false)?;
    Some(i)
}
// }}}
