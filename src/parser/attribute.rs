extern crate alloc;

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;

use alloc::borrow::Cow;
use alloc::string::String;

use crate::ast::Attributes;
use crate::text::{self};
use crate::util::{
    is_punct, is_space, resolve_entity_references, resolve_numeric_references, TinyVec,
};

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
                attrs.insert(name, (s + " " + &value.str(reader.source())).into());
            } else {
                attrs.insert(name, value);
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

fn parse_attribute<'a>(
    reader: &mut impl text::Reader<'a>,
) -> Option<(String, text::MultilineValue)> {
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

fn parse_attribute_value<'a>(reader: &mut impl text::Reader<'a>) -> Option<text::MultilineValue> {
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
) -> Option<text::MultilineValue> {
    reader.advance(1); // skip a opening quote
    let mut value = TinyVec::<text::Index>::empty();
    let mut break_loop = false;
    loop {
        let (line, mut s) = reader.peek_line_bytes()?;
        if let Some(i) = memchr::memchr(q, &line) {
            reader.advance(i + 1);
            s = s.with_stop(s.start() + i);
            break_loop = true;
        } else {
            reader.advance_line();
        }
        value.push(s.into());

        if break_loop {
            break;
        }
    }
    if !break_loop {
        return None;
    }
    if value.len() == 1 {
        // fast path
        let resolved =
            resolve_numeric_references(resolve_entity_references(value[0].bytes(reader.source())));
        Some(match resolved {
            Cow::Borrowed(_) => value.into(),
            Cow::Owned(s) => s.into(),
        })
    } else {
        let mut result = String::new();
        let mut has_resolved = false;
        for idx in &value {
            let resolved =
                resolve_numeric_references(resolve_entity_references(idx.bytes(reader.source())));
            result.push_str(unsafe { core::str::from_utf8_unchecked(&resolved) });
            if matches!(resolved, Cow::Owned(_)) {
                has_resolved = true;
            }
        }
        if has_resolved {
            Some(result.into())
        } else {
            Some(value.into())
        }
    }
}

fn parse_unquoted_attribute_value<'a>(
    reader: &mut impl text::Reader<'a>,
) -> Option<text::MultilineValue> {
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
    let resolved = resolve_numeric_references(resolve_entity_references(s.bytes(reader.source())));
    Some(match resolved {
        Cow::Borrowed(_) => s.into(),
        Cow::Owned(s) => s.into(),
    })
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
        assert!(matches!(
            attrs.get("id").unwrap(),
            &text::MultilineValue::Indices(_)
        ));
        assert_eq!(attrs.get("class").unwrap().str(source), "class1 class2");
        assert!(matches!(
            attrs.get("class").unwrap(),
            &text::MultilineValue::String(_)
        ));
        assert_eq!(attrs.get("title").unwrap().str(source), "My &Title");
        assert!(matches!(
            attrs.get("title").unwrap(),
            &text::MultilineValue::String(_)
        ));
        assert_eq!(attrs.get("attr").unwrap().str(source), "aaa");
        assert!(matches!(
            attrs.get("attr").unwrap(),
            &text::MultilineValue::Indices(_)
        ));

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
            &text::MultilineValue::Indices(_)
        ));
        let (line, _) = reader.peek_line().unwrap();
        assert_eq!(line.as_ref(), " rest");
    }
}

// }}} Tests
