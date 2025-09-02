use super::*;
#[cfg(test)]
mod tests {
    use super::*;
    use markdown::{to_mdast, ParseOptions};

    #[test]
    fn test_simple_statements_count() {
        let input = r#"This is a statement with a number: 42.

- First item

- Second item with [a link](http://example.com)
  - Nested item
"#;

        // Inline parsing
        let root_node = to_mdast(input, &ParseOptions::default()).unwrap();
        let doc = DokeBaseParser::parse_document(&root_node);

        // Only assert the top-level statements count
        assert_eq!(doc.statements.len(), 4);

        // Optionally assert their text slices
        let slices: Vec<&str> = doc.statements.iter()
            .map(|stmt| stmt.statement_position.as_ref()
                 .map(|p| &input[p.start..p.end])
                 .unwrap_or(""))
            .collect();

        assert_eq!(slices.len(), 4);
    }
}
