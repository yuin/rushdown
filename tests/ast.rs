use rushdown::{node_path, parser, text};

fn test_inline_pos_aux(source: &str, node_type: &str) {
    let p = parser::Parser::with_extensions(
        parser::Options::default(),
        parser::gfm(parser::GfmOptions::default()),
    );
    let mut reader = text::BasicReader::new(source);
    let (arena, document_ref) = p.parse(&mut reader);
    let n = node_path!(arena, document_ref, first_child, first_child).unwrap();
    assert_eq!(
        arena[n].pos(),
        Some(1),
        "failed to get correct node position({})",
        node_type
    );
    #[cfg(feature = "pp-ast")]
    {
        use rushdown::ast::pretty_print;
        let mut w = String::new();
        pretty_print(&mut w, &arena, document_ref, source).expect("failed to pretty print");
        println!("{}", w);
    }
}

#[test]
fn test_inline_pos() {
    test_inline_pos_aux(
        r#"
text
"#,
        "Text",
    );
    test_inline_pos_aux(
        r#"
`code span`
"#,
        "CodeSpan",
    );

    test_inline_pos_aux(
        r#"
***emphasis**
"#,
        "Emphasis",
    );

    test_inline_pos_aux(
        r#"
[aaa](https://example.com)
"#,
        "Inline link",
    );

    test_inline_pos_aux(
        r#"
[bbb][aaa]

[aaa]: https://example.com
"#,
        "Full reference link",
    );

    test_inline_pos_aux(
        r#"
[aaa][]

[aaa]: https://example.com
"#,
        "Collapsed reference link",
    );

    test_inline_pos_aux(
        r#"
[aaa]

[aaa]: https://example.com
"#,
        "Shortcut reference link",
    );

    test_inline_pos_aux(
        r#"
![aaa](https://example.com/image.png)
"#,
        "Image",
    );

    test_inline_pos_aux(
        r#"
<a href="https://example.com">link</a>
"#,
        "RawHtml",
    );
    test_inline_pos_aux(
        r#"
~~aa~~
"#,
        "Strikethrough",
    );
}

fn test_block_pos_aux(source: &str, node_type: &str, expected: usize, n: usize) {
    let p = parser::Parser::with_extensions(
        parser::Options::default(),
        parser::gfm(parser::GfmOptions::default()),
    );
    let mut reader = text::BasicReader::new(source);
    let (arena, document_ref) = p.parse(&mut reader);

    let mut fc = arena[document_ref].first_child().unwrap();
    for _ in 0..n {
        fc = arena[fc].first_child().unwrap();
    }
    assert_eq!(
        arena[fc].pos(),
        Some(expected),
        "failed to get correct node position({})",
        node_type
    );

    #[cfg(feature = "pp-ast")]
    {
        use rushdown::ast::pretty_print;
        let mut w = String::new();
        pretty_print(&mut w, &arena, document_ref, source).expect("failed to pretty print");
        println!("{}", w);
    }
}

#[test]
fn test_block_pos() {
    test_block_pos_aux(
        r#"
aaa
"#,
        "Paragraph",
        1,
        0,
    );

    test_block_pos_aux(
        r#"
# Heading
"#,
        "AtxHeading",
        1,
        0,
    );

    test_block_pos_aux(
        r#"
Heading
===
"#,
        "SetextHeading",
        1,
        0,
    );

    test_block_pos_aux(
        r#"
------
"#,
        "ThematicBreak",
        1,
        0,
    );
    test_block_pos_aux(
        r#"
    aaaa
    bbbb
"#,
        "IndentedCodeBlock",
        5,
        0,
    );
    test_block_pos_aux(
        r#"
```
aaa
bbb
```"#,
        "FencedCodeBlock",
        1,
        0,
    );
    test_block_pos_aux(
        r#"
> blockquote
"#,
        "BlockQuote",
        1,
        0,
    );
    test_block_pos_aux(
        r#"
- list item
"#,
        "List",
        1,
        0,
    );
    test_block_pos_aux(
        r#"
- list item
"#,
        "ListItem",
        1,
        1,
    );
    test_block_pos_aux(
        r#"
<!--

aaaa

-->
"#,
        "HtmlBlock",
        1,
        0,
    );
    test_block_pos_aux(
        r#"
[aaa]: https://example.com

[aaa]
"#,
        "LinkReferenceDefinition",
        1,
        0,
    );

    let source = r#"
| aaa | bbb |
| --- | --- |
| ccc | ddd |
"#;
    let p = parser::Parser::with_extensions(
        parser::Options::default(),
        parser::gfm(parser::GfmOptions::default()),
    );
    let mut reader = text::BasicReader::new(source);
    let (mut arena, document_ref) = p.parse(&mut reader);

    {
        let n = node_path!(&mut arena, document_ref, first_child).unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "Table");
        assert_eq!(arena[n].pos(), Some(1));
    }
    {
        let n = node_path!(&mut arena, document_ref, first_child, first_child).unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "TableHeader");
        assert_eq!(arena[n].pos(), Some(1));
    }
    {
        let n = node_path!(
            &mut arena,
            document_ref,
            first_child,
            first_child,
            first_child
        )
        .unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "TableRow");
        assert_eq!(arena[n].pos(), Some(1));
    }
    {
        let n = node_path!(
            &mut arena,
            document_ref,
            first_child,
            first_child,
            first_child,
            first_child
        )
        .unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "TableCell");
        assert_eq!(arena[n].pos(), Some(1));
    }
    {
        let n = node_path!(
            &mut arena,
            document_ref,
            first_child,
            first_child,
            first_child,
            first_child,
            next_sibling
        )
        .unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "TableCell");
        assert_eq!(arena[n].pos(), Some(7));
    }
    {
        let n = node_path!(
            &mut arena,
            document_ref,
            first_child,
            first_child,
            next_sibling
        )
        .unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "TableBody");
        assert_eq!(arena[n].pos(), Some(29));
    }
    {
        let n = node_path!(
            &mut arena,
            document_ref,
            first_child,
            first_child,
            next_sibling,
            first_child
        )
        .unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "TableRow");
        assert_eq!(arena[n].pos(), Some(29));
    }
    {
        let n = node_path!(
            &mut arena,
            document_ref,
            first_child,
            first_child,
            next_sibling,
            first_child,
            first_child
        )
        .unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "TableCell");
        assert_eq!(arena[n].pos(), Some(29));
    }
    {
        let n = node_path!(
            &mut arena,
            document_ref,
            first_child,
            first_child,
            next_sibling,
            first_child,
            first_child,
            next_sibling
        )
        .unwrap();
        assert_eq!(arena[n].kind_data().kind_name(), "TableCell");
        assert_eq!(arena[n].pos(), Some(35));
    }
}
