use markdown::mdast::Node;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, DokeParseError>;

#[derive(Error, Debug)]
pub enum DokeParseError {
    #[error("YAML parsing error: {0}")]
    YamlParseError(#[from] yaml_rust2::scanner::ScanError),
    #[error("YAML conversion error: {0}")]
    YamlConversionError(String),
    #[error("Invalid node structure: {0}")]
    InvalidNodeStructure(String),
    #[error("Position data missing for node")]
    MissingPositionData,
    #[error("Unexpected node type: expected {expected}, found {actual}")]
    UnexpectedNodeType {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Full document
#[derive(Debug)]
pub struct DokeBaseDocument<'a> {
    pub statements: Vec<DokeStatement<'a>>,
    pub frontmatter: Option<yaml_rust2::Yaml>,
}

/// Position in the source string
#[derive(Debug, Clone)]
pub struct Position {
    pub start: usize,
    pub end: usize,
}

impl Position {
    pub fn merge(&self, other: &Position) -> Position {
        Position {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Fenced or inline code block info
#[derive(Debug, Clone)]
pub struct CodeBlock<'a> {
    pub content: &'a str,
    pub language: Option<&'a str>,
    pub position: Position,
}

/// Logical statement in the Doke document
#[derive(Debug, Clone)]
pub struct DokeStatement<'a> {
    pub node: &'a markdown::mdast::Node,
    pub children: Vec<DokeStatement<'a>>,
    pub statement_position: Option<Position>,
    pub full_position: Option<Position>,
    pub children_position: Option<Position>,
    pub code_blocks: Vec<CodeBlock<'a>>,
}

pub struct DokeBaseParser;

impl DokeBaseParser {
    fn convert_position(pos: &markdown::unist::Position) -> Position {
        Position {
            start: pos.start.offset,
            end: pos.end.offset,
        }
    }

    /// Parse a document from the root AST node
    pub fn parse_document<'a>(
        root: &'a Node,
        frontmatter_string: Option<&str>,
    ) -> Result<DokeBaseDocument<'a>> {
        let mut frontmatter: Option<yaml_rust2::Yaml> = None;
        if let Some(frontmatter_str) = frontmatter_string {
            let docs = yaml_rust2::YamlLoader::load_from_str(frontmatter_str).unwrap_or(vec![]);
            if !docs.is_empty() {
                frontmatter = Some(docs[0].clone());
            }
        }

        let mut statements = Vec::new();
        if let Some(children) = root.children() {
            statements.extend(Self::parse_sibling_blocks(children));
        }

        Ok(DokeBaseDocument {
            statements,
            frontmatter,
        })
    }

    /// Parse a slice of sibling nodes into a tree of statements
    fn parse_sibling_blocks<'a>(siblings: &'a [Node]) -> Vec<DokeStatement<'a>> {
        let mut stmts = Vec::new();
        let mut i = 0;

        while i < siblings.len() {
            let child = &siblings[i];
            match child {
                Node::Paragraph(_) | Node::Heading(_) | Node::Code(_) => {
                    let mut stmt = Self::parse_statement_node(child);

                    // Attach any following list nodes as children
                    let mut j = i + 1;
                    while j < siblings.len() {
                        if let Node::List(_) = &siblings[j] {
                            if let Some(list_items) = siblings[j].children() {
                                for item in list_items {
                                    if let Some(child_stmt) = Self::parse_list_item(item) {
                                        stmt.children.push(child_stmt);
                                    }
                                }
                            }
                            j += 1;
                        } else {
                            break;
                        }
                    }

                    stmt.children_position = stmt
                        .children
                        .iter()
                        .filter_map(|c| c.full_position.clone())
                        .reduce(|a, b| a.merge(&b));

                    stmts.push(stmt);
                    i = j;
                }
                Node::List(_) => {
                    if let Some(list_items) = child.children() {
                        for item in list_items {
                            if let Some(stmt) = Self::parse_list_item(item) {
                                stmts.push(stmt);
                            }
                        }
                    }
                    i += 1;
                }
                Node::ListItem(_) => {
                    if let Some(stmt) = Self::parse_list_item(child) {
                        stmts.push(stmt);
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }

        stmts
    }

    fn parse_statement_node<'a>(node: &'a Node) -> DokeStatement<'a> {
        let mut code_blocks = Vec::new();

        if let Node::Code(code) = node {
            let pos = node.position().map(Self::convert_position);
            code_blocks.push(CodeBlock {
                content: &code.value,
                language: code.lang.as_deref(),
                position: pos.clone().unwrap_or(Position {
                    start: 0,
                    end: code.value.len(),
                }),
            });
        }

        Self::collect_inline_code_blocks(node, &mut code_blocks);

        let statement_position = Self::merge_inline_positions(node);

        DokeStatement {
            node,
            children: Vec::new(),
            statement_position,
            full_position: node.position().map(Self::convert_position),
            children_position: None,
            code_blocks,
        }
    }

    fn parse_list_item<'a>(item: &'a Node) -> Option<DokeStatement<'a>> {
        assert!(matches!(item, Node::ListItem(_)));

        if let Some(kids) = item.children() {
            let substmts = Self::parse_sibling_blocks(kids);
            if !substmts.is_empty() {
                let mut first = substmts[0].clone();
                first.children.extend(substmts.into_iter().skip(1));
                first.children_position = first
                    .children
                    .iter()
                    .filter_map(|c| c.full_position.clone())
                    .reduce(|a, b| a.merge(&b));
                return Some(first);
            }
        }

        None
    }

    fn collect_inline_code_blocks<'a>(node: &'a Node, code_blocks: &mut Vec<CodeBlock<'a>>) {
        if let Node::InlineCode(code) = node {
            let pos = node.position().map(Self::convert_position);
            code_blocks.push(CodeBlock {
                content: &code.value,
                language: None,
                position: pos.unwrap_or(Position {
                    start: 0,
                    end: code.value.len(),
                }),
            });
        }

        if let Some(children) = node.children() {
            for child in children {
                Self::collect_inline_code_blocks(child, code_blocks);
            }
        }
    }

    fn merge_inline_positions(node: &Node) -> Option<Position> {
        let mut merged: Option<Position> = None;

        fn recurse(n: &Node, acc: &mut Option<Position>) {
            if let Some(pos) = n.position() {
                let p = DokeBaseParser::convert_position(pos);
                *acc = Some(match acc {
                    Some(existing) => existing.merge(&p),
                    None => p,
                });
            }
            if let Some(children) = n.children() {
                for child in children {
                    recurse(child, acc);
                }
            }
        }

        recurse(node, &mut merged);
        merged
    }
}
