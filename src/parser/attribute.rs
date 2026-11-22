extern crate alloc;

use alloc::string::String;

#[allow(unused_imports)]
#[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
use crate::println;
use crate::{
    ast::Attributes,
    text::{self},
    util::{is_punct, is_space, TinyVec},
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
                attrs.insert(
                    name,
                    text::Value::from_string(
                        s + " " + &value.str(reader.source()),
                        reader.decoder(),
                    ),
                );
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

fn parse_attribute<'a>(reader: &mut impl text::Reader<'a>) -> Option<(String, text::Value)> {
    reader.skip_spaces();
    let c = reader.peek_byte();
    if c == b'#' || c == b'.' {
        reader.advance(1);
        let (line, seg) = reader.peek_line_bytes()?;
        // HTML5 allows any kind of characters as id, but XHTML restricts characters for id.
        // CommonMark is basically defined for XHTML(even though it is legacy).
        // So we restrict id characters.
        let i = line
            .iter()
            .take_while(|&&b| {
                !is_space(b) && (!is_punct(b) || b == b'_' || b == b'-' || b == b':' || b == b'.')
            })
            .count();
        if i == 0 {
            return None;
        }
        reader.advance(i);
        if c == b'#' {
            return Some((
                "id".into(),
                text::Value::from_segment(seg.with_stop(seg.start() + i), reader.decoder()),
            ));
        }
        return Some((
            "class".into(),
            text::Value::from_segment(seg.with_stop(seg.start() + i), reader.decoder()),
        ));
    }
    let (line, seg) = reader.peek_line_bytes()?;
    if line.is_empty() {
        return None;
    }
    if invalid_html_attribute_name_byte(line[0]) {
        return None;
    }
    let i = line
        .iter()
        .take_while(|&&b| !invalid_html_attribute_name_byte(b))
        .count();
    let name = &line[0..i];
    reader.advance(i);
    reader.skip_spaces();
    if reader.peek_byte() != b'=' {
        return Some((
            String::from_utf8_lossy(name).into_owned(),
            text::Value::from_segment(seg.with_stop(seg.start() + i), reader.decoder()),
        ));
    }
    reader.advance(1); // skip '='
    let value = parse_attribute_value(reader)?;
    Some((String::from_utf8_lossy(name).into_owned(), value))
}

fn invalid_html_attribute_name_byte(b: u8) -> bool {
    b <= 0x20 || b == 0x7f || matches!(b, b'"' | b'\'' | b'<' | b'>' | b'/' | b'=' | b'}')
}

fn parse_attribute_value<'a>(reader: &mut impl text::Reader<'a>) -> Option<text::Value> {
    reader.skip_spaces();
    match reader.peek_byte() {
        text::EOS => None,
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
    Some(text::Value::from_indices(value, reader.decoder()))
}

fn parse_unquoted_attribute_value<'a>(reader: &mut impl text::Reader<'a>) -> Option<text::Value> {
    let (line, mut s) = reader.peek_line_bytes()?;
    // `"`, `\`, `>`, `<`, `=`, `` ` `` and `'` are not allowed in unquoted HTML
    // attribute values, but most of implementations ignore this rule, so we do too.
    let i = line
        .iter()
        .take_while(|&&b| !is_space(b) && b != b'}')
        .count();
    reader.advance(i);
    s = s.with_stop(s.start() + i);
    Some(text::Value::from_segment(s, reader.decoder()))
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
            &text::Value::Index(_, _)
        ));
        assert_eq!(attrs.get("class").unwrap().str(source), "class1 class2");
        assert!(matches!(
            attrs.get("class").unwrap(),
            &text::Value::String(_, _)
        ));
        assert_eq!(attrs.get("title").unwrap().str(source), "My &amp;Title");
        assert!(matches!(
            attrs.get("title").unwrap(),
            &text::Value::Index(_, _)
        ));
        assert_eq!(attrs.get("title").unwrap().value(source), "My &Title");
        assert_eq!(attrs.get("attr").unwrap().str(source), "aaa");
        assert!(matches!(
            attrs.get("attr").unwrap(),
            &text::Value::Index(_, _)
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
            &text::Value::Indices(_, _)
        ));
        let (line, _) = reader.peek_line().unwrap();
        assert_eq!(line.as_ref(), " rest");
    }

    #[test]
    fn test_parse_attributes_name_grammar() {
        let source = "{data-attr}";
        let mut reader = text::BasicReader::new(source);
        let attrs = parse_attributes(&mut reader).unwrap();
        assert_eq!(attrs.get("data-attr").unwrap().str(source), "data-attr");

        let source = "{k@y=1}";
        let mut reader = text::BasicReader::new(source);
        let attrs = parse_attributes(&mut reader).unwrap();
        assert_eq!(attrs.get("k@y").unwrap().str(source), "1");

        let source = "{a/b=2}";
        let mut reader = text::BasicReader::new(source);
        assert!(parse_attributes(&mut reader).is_none());
    }

    #[test]
    fn test_parse_attributes_unquoted_value_stops() {
        let source = "{a=[1,2] b=x\"y}";
        let mut reader = text::BasicReader::new(source);
        let attrs = parse_attributes(&mut reader).unwrap();
        assert_eq!(attrs.get("a").unwrap().str(source), "[1,2]");
        assert_eq!(attrs.get("b").unwrap().str(source), "x\"y");
    }
}

// }}} Tests
