//! Scanners for various patterns in text.

mod scanner_gen;

pub use self::scanner_gen::*;

use crate::text::{self, Reader, Segment, EOS};
use crate::util::is_space;
use memchr::memmem;

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

fn scan_open_close<'a>(
    reader: &mut impl text::Reader<'a>,
    open: &[u8],
    close: &[u8],
) -> Option<()> {
    let (line, _) = reader.peek_line_bytes()?;
    let (l, p) = reader.position();
    if line.starts_with(open) {
        reader.advance(open.len());
        loop {
            let Some((line, _)) = reader.peek_line_bytes() else {
                reader.set_position(l, p);
                return None;
            };
            if let Some(pos) = memmem::find(&line, close) {
                reader.advance(pos + close.len());
                return Some(());
            }
            reader.advance(line.len());
        }
    }
    reader.set_position(l, p);
    None
}

// }}}

// HTML scanners {{{

include!(concat!(env!("OUT_DIR"), "/allowed_block_tags.rs"));

pub(crate) fn scan_html_block_open_1(line: &[u8]) -> Option<()> {
    let mut i = not_indented(line)?;
    i += starts_with(line, i, &["<textarea", "<script", "<pre", "<style"], false)?;
    starts_with(line, i, &["/>", "\r\n", " ", "\t", ">", "\n"], true)?;
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
        && tag_name != "pre"
        && tag_name != "script"
        && tag_name != "style"
        && tag_name != "textarea"
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
    i += starts_with(line, i, &["/>", ">"], false)?;
    i += skip_spaces(line, i);
    (i == line.len()).then_some(())
}

pub(crate) fn scan_html_attributes(s: &[u8]) -> Option<usize> {
    let mut r = unsafe { text::BasicReader::new_unchecked(s) };
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
        if c == b'/' {
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

pub(crate) fn scan_html_comment_reader<'a>(reader: &mut impl text::Reader<'a>) -> Option<()> {
    let (line, _) = reader.peek_line_bytes()?;
    if line.starts_with(b"<!-->") {
        reader.advance(5);
        return Some(());
    }
    if line.starts_with(b"<!--->") {
        reader.advance(6);
        return Some(());
    }
    scan_open_close(reader, b"<!--", b"-->")
}

pub(crate) fn scan_html_processing_instruction_reader<'a>(
    reader: &mut impl text::Reader<'a>,
) -> Option<()> {
    scan_open_close(reader, b"<?", b"?>")
}

pub(crate) fn scan_html_declaration_reader<'a>(reader: &mut impl text::Reader<'a>) -> Option<()> {
    let (line, _) = reader.peek_line_bytes()?;
    if !line.starts_with(b"<!") {
        return None;
    }
    let (l, p) = reader.position();
    reader.advance(2);
    if !reader.peek_byte().is_ascii_alphabetic() {
        reader.set_position(l, p);
        return None;
    }
    reader.advance(1);
    reader.skip_while(|c| c != b'>');
    if reader.peek_byte() != b'>' {
        reader.set_position(l, p);
        return None;
    }
    reader.advance(1);
    Some(())
}

pub(crate) fn scan_html_cdata_reader<'a>(reader: &mut impl text::Reader<'a>) -> Option<()> {
    scan_open_close(reader, b"<![CDATA[", b"]]>")
}

// }}} HTML_block scanners

// URL scanners {{{

pub(crate) fn scan_url_www(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    i += scan_url_www_domain(buffer)?;
    match scan_url_path(&buffer[i..]) {
        Some(m) => i += m,
        None => return None,
    }
    Some(i)
}

pub(crate) fn scan_url_strict(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    i += skip_while(buffer, 0, |c| c.is_ascii_lowercase(), false)?;
    i += starts_with(buffer, i, &["://"], false)?;
    i += scan_url_domain(&buffer[i..])?;
    match scan_url_path(&buffer[i..]) {
        Some(m) => i += m,
        None => return None,
    }
    Some(i)
}

fn scan_url_www_domain(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    i += starts_with(buffer, 0, &["www"], false)?;
    let mut last_start = 0usize;
    let mut last_stop = 0usize;
    let mut n = 0;
    while i < buffer.len() {
        if buffer[i] != b'.'
            && (!is_domain_safe(buffer[i])
                || buffer[i] != b'/' && buffer[i] != b'#' && buffer[i] != b'?')
        {
            break;
        }
        i += starts_with(buffer, i, &["."], false)?;
        match skip_while(buffer, i, is_domain_safe, false) {
            Some(m) => {
                last_start = i;
                i += m;
                n += 1;
                last_stop = i;
            }
            None => break,
        }
    }
    if n < 2 || matches_tld_and_port(&buffer[last_start..last_stop]).is_none() {
        return None;
    }
    Some(i)
}

fn is_domain_safe(c: u8) -> bool {
    c == b'-'
        || c == b'@'
        || c == b':'
        || c == b'%'
        || c == b'_'
        || c == b'+'
        || c == b'~'
        || c == b'='
        || c.is_ascii_alphanumeric()
}

fn scan_url_domain(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    i += skip_while(buffer, 0, is_domain_safe, false)?;
    let mut last_start = 0usize;
    let mut last_stop = 0usize;
    let mut n = 0;
    while i < buffer.len() {
        if buffer[i] != b'.'
            && (!is_domain_safe(buffer[i])
                || buffer[i] != b'/' && buffer[i] != b'#' && buffer[i] != b'?')
        {
            break;
        }
        i += starts_with(buffer, i, &["."], false)?;
        match skip_while(buffer, i, is_domain_safe, false) {
            Some(m) => {
                last_start = i;
                i += m;
                n += 1;
                last_stop = i;
            }
            None => break,
        }
    }

    if n < 1 || matches_tld_and_port(&buffer[last_start..last_stop]).is_none() {
        return None;
    }
    Some(i)
}

fn matches_tld_and_port(buffer: &[u8]) -> Option<()> {
    let mut i = 0;
    skip_while(buffer, i, |c| c.is_ascii_lowercase(), false)?;
    if i < buffer.len() && buffer[i] == b':' {
        i += 1;
        skip_while(buffer, i, |c| c.is_ascii_digit(), false)?;
    }
    Some(())
}

fn scan_url_path(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    match starts_with(buffer, 0, &["/", "#", "?"], true) {
        Some(m) => i += m,
        None => return Some(0),
    }

    match skip_while(
        buffer,
        i,
        |c| {
            is_domain_safe(c)
                || c == b'('
                || c == b')'
                || c == b';'
                || c == b','
                || c == b'\''
                || c == b'"'
                || c == b'>'
                || c == b'^'
                || c == b'{'
                || c == b'}'
                || c == b'['
                || c == b']'
                || c == b'`'
                || c == b'!'
                || c == b'?'
                || c == b'$'
                || c == b'&'
                || c == b'~'
                || c == b'/'
        },
        true,
    ) {
        Some(m) => i += m,
        None => i += 0,
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
