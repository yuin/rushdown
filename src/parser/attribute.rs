extern crate alloc;

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;

use alloc::borrow::Cow;
use alloc::string::String;

use crate::ast::Attributes;
use crate::text::{self, Segment};
use crate::util::{is_punct, is_space, resolve_entity_references, resolve_numeric_references};

/// Parses attributes from the reader.
/// If attributes are found, returns Some(Attributes).
/// Otherwise, returns None and resets the reader position
///
/// Attributes are expected to be in the format almost like HTML attributes,
/// enclosed in curly braces `{}`. For example:
///
/// `{#id .class key="value" another='value2' unquoted=value3 q="&quot;value&quot;"}`
///
/// Supports shorthand for `id` and `class`. `#id` is equivalent to `id="id"`,
/// and `.class` is equivalent to `class="class"`. Multiple classes can be specified
/// by repeating the `.class` shorthand, which will be concatenated with spaces.
pub fn parse_attributes<'a>(reader: &mut impl text::Reader<'a>) -> Option<Attributes> {
    let (saved_line, saved_position) = reader.position();
    reader.skip_spaces();
    if reader.peek_byte() != b'{' {
        reader.set_position(saved_line, saved_position);
        return None;
    }
    reader.advance(1);
    let mut attrs = Attributes::new();
    loop {
        if reader.peek_byte() == b'}' {
            reader.advance(1);
            return Some(attrs);
        }
        if let Some((name, value)) = parse_attribute(reader) {
            if name == "class" && attrs.contains_key("class") {
                let s = String::from(attrs.get("class").unwrap().str(reader.source()));
                attrs.set(name, s + " " + value.str(reader.source()));
            } else {
                attrs.set(name, value);
            }
            reader.skip_spaces();
            if reader.peek_byte() == b',' {
                reader.advance(1);
            }
            reader.skip_spaces();
        } else {
            reader.set_position(saved_line, saved_position);
            return None;
        }
    }
}

fn parse_attribute<'a>(reader: &mut impl text::Reader<'a>) -> Option<(String, text::Value)> {
    reader.skip_spaces();
    let c = reader.peek_byte();
    if c == b'#' || c == b'.' {
        reader.advance(1);
        let (line, seg) = reader.peek_line_bytes()?;
        if line.is_empty() {
            return None;
        }
        // HTML5 allows any kind of characters as id, but XHTML restricts characters for id.
        // CommonMark is basically defined for XHTML(even though it is legacy).
        // So we restrict id characters.
        let i = line
            .iter()
            .take_while(|&&b| {
                !is_space(b) && (!is_punct(b) || b == b'_' || b == b'-' || b == b':' || b == b'.')
            })
            .count();
        reader.advance(i);
        if c == b'#' {
            return Some(("id".into(), seg.with_stop(seg.start() + i).into()));
        }
        return Some(("class".into(), seg.with_stop(seg.start() + i).into()));
    }
    let (line, _) = reader.peek_line_bytes()?;
    if line.is_empty() {
        return None;
    }
    let c = line[0];
    if !(c.is_ascii_alphabetic() || c == b'_' || c == b':') {
        return None;
    }
    let i = line
        .iter()
        .take_while(|&&b| {
            b.is_ascii_alphabetic()
                || b.is_ascii_digit()
                || b == b'_'
                || b == b'-'
                || b == b':'
                || b == b'.'
        })
        .count();
    let name = &line[0..i];
    reader.advance(i);
    reader.skip_spaces();
    if reader.peek_byte() != b'=' {
        return None;
    }
    reader.advance(1); // skip '='
    let value = parse_attribute_value(reader)?;
    Some((String::from_utf8_lossy(name).into_owned(), value))
}

fn parse_attribute_value<'a>(reader: &mut impl text::Reader<'a>) -> Option<text::Value> {
    reader.skip_spaces();
    match reader.peek_byte() {
        b'"' => parse_quoted_attribute_value(reader, b'"'),
        b'\'' => parse_quoted_attribute_value(reader, b'\''),
        _ => parse_unquoted_attribute_value(reader),
    }
}

fn parse_quoted_attribute_value<'a>(
    reader: &mut impl text::Reader<'a>,
    q: u8,
) -> Option<text::Value> {
    reader.advance(1); // skip a opening quote
    let mut seg = Segment::new(0, 0);
    let mut sv: Option<String> = None;
    loop {
        let (line, mut s) = reader.peek_line_bytes()?;
        let mut break_loop = false;
        if let Some(i) = line.iter().position(|&b| b == q) {
            reader.advance(i + 1);
            s = s.with_stop(s.start() + i);
            break_loop = true;
        } else {
            reader.advance_line();
        }
        if seg.is_empty() {
            seg = s;
        } else if seg.stop() == s.start() {
            seg = seg.with_stop(s.stop());
        } else {
            match sv {
                Some(ref mut string_value) => {
                    string_value.push_str(s.str(reader.source()).as_ref());
                }
                None => {
                    let mut string_value = String::from(seg.str(reader.source()));
                    string_value.push_str(s.str(reader.source()).as_ref());
                    sv = Some(string_value);
                }
            }
        }

        if break_loop {
            break;
        }
    }
    if let Some(s) = sv {
        let mut cw = resolve_entity_references(s.as_bytes());
        cw = resolve_numeric_references(cw);
        Some(cw.as_ref().into())
    } else {
        let s = seg.str(reader.source());
        let mut cw = resolve_entity_references(s.as_bytes());
        cw = resolve_numeric_references(cw);
        match cw {
            Cow::Borrowed(_) => Some(seg.into()),
            Cow::Owned(s) => Some(s.into()),
        }
    }
}

fn parse_unquoted_attribute_value<'a>(reader: &mut impl text::Reader<'a>) -> Option<text::Value> {
    let (line, mut s) = reader.peek_line_bytes()?;
    let i = line
        .iter()
        .take_while(|&&b| {
            !is_space(b)
                && b != b'}'
                && b != b'"'
                && b != b'\''
                && b != b'='
                && b != b'<'
                && b != b'>'
                && b != b'`'
                && b != b','
        })
        .count();

    if i == 0 {
        return None;
    }
    reader.advance(i);
    s = s.with_stop(s.start() + i);
    let s_str = s.str(reader.source());
    let mut cw = resolve_entity_references(s_str.as_bytes());
    cw = resolve_numeric_references(cw);
    match cw {
        Cow::Borrowed(_) => Some(s.into()),
        Cow::Owned(s) => Some(s.into()),
    }
}

// Tests {{{

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(unused_imports)]
    #[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
    use crate::println;

    use crate::text::Reader;

    #[test]
    fn test_parse_attributes() {
        let source = "{#my-id .class1 .class2 title=\"My &amp;Title\" attr=aaa} rest of line";
        let mut reader = text::BasicReader::new(source);
        let attrs = parse_attributes(&mut reader).unwrap();
        assert_eq!(attrs.get("id").unwrap().str(source), "my-id");
        assert!(matches!(attrs.get("id").unwrap(), &text::Value::Index(_)));
        assert_eq!(attrs.get("class").unwrap().str(source), "class1 class2");
        assert!(matches!(
            attrs.get("class").unwrap(),
            &text::Value::String(_)
        ));
        assert_eq!(attrs.get("title").unwrap().str(source), "My &Title");
        assert!(matches!(
            attrs.get("title").unwrap(),
            &text::Value::String(_)
        ));
        assert_eq!(attrs.get("attr").unwrap().str(source), "aaa");
        assert!(matches!(attrs.get("attr").unwrap(), &text::Value::Index(_)));

        let (line, _) = reader.peek_line().unwrap();
        assert_eq!(line.as_ref(), " rest of line");
    }

    #[test]
    fn test_parse_attributes_multiline() {
        let source = "{title=\"This is a \nmultiline title\"} rest";
        let mut reader = text::BasicReader::new(source);
        let attrs = parse_attributes(&mut reader).unwrap();
        assert_eq!(
            attrs.get("title").unwrap().str(source),
            "This is a \nmultiline title"
        );
        assert!(matches!(
            attrs.get("title").unwrap(),
            &text::Value::Index(_)
        ));
        let (line, _) = reader.peek_line().unwrap();
        assert_eq!(line.as_ref(), " rest");
    }
}

// }}} Tests
