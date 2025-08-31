//! # DokeParser
//!
//! Parses a Markdown-like Doke file into `DokeStatement`s while tracking positions for content and children.

use markdown::{ParseOptions, mdast::Node};
use std::fmt;

#[derive(Debug, Clone)]
pub struct DokeDocument {
    pub statements: Vec<DokeStatement>,
}

#[derive(Debug, Clone)]
pub struct DokeStatement {
    pub children: Vec<DokeStatement>,
    pub content_position: Option<(usize, usize)>, // text only, including inline code/links/emphasis
    pub position: Option<(usize, usize)>,         // full statement including children
    pub children_position: Option<(usize, usize)>, // full children text
}

#[derive(Debug, thiserror::Error)]
pub enum DokeParseError {
    #[error("Markdown parsing error: {0}")]
    MarkdownParseError(String),
    #[error("Invalid document structure")]
    InvalidStructure,
}

pub struct DokeParser {
    options: ParseOptions,
}

impl Default for DokeParser {
    fn default() -> Self {
        Self::new()
    }
}

impl DokeParser {
    pub fn new() -> Self {
        Self {
            options: ParseOptions::default(),
        }
    }

    pub fn parse(&self, input: &str) -> Result<DokeDocument, DokeParseError> {
        let root_node: Node = markdown::to_mdast(input, &self.options)
            .map_err(|e| DokeParseError::MarkdownParseError(e.to_string()))?;
        let statements: Vec<DokeStatement> = self.extract_statements_with_hierarchy(&root_node, input)?;
        Ok(DokeDocument { statements })
    }

    fn extract_statements_with_hierarchy(
        &self,
        node: &Node,
        source: &str,
    ) -> Result<Vec<DokeStatement>, DokeParseError> {
        let mut statements: Vec<DokeStatement> = Vec::new();
        if let Node::Root(root) = node {
            let mut previous_index: Option<usize> = None;
            for child in &root.children {
                match child {
                    Node::Paragraph(_) => {
                        let content_position = self.get_clean_content_position(child, source);
                        let full_position = child.position().map(|p| (p.start.offset, p.end.offset));
                        statements.push(DokeStatement {
                            children: Vec::new(),
                            content_position,
                            position: full_position,
                            children_position: None,
                        });
                        previous_index = if let Some((start, end)) = content_position {
                            if source[start..end].trim_end().ends_with(':') {
                                Some(statements.len() - 1)
                            } else {
                                None
                            }
                        } else { None };
                    }
                    Node::List(_) => {
                        if let Some(prev_idx) = previous_index {
                            let (children, children_pos) = self.extract_list_children(child, source)?;
                            if let Some(prev_stmt) = statements.get_mut(prev_idx) {
                                prev_stmt.children.extend(children);
                                prev_stmt.children_position = self.merge_positions(prev_stmt.children_position, children_pos);
                                if let Some((start, end)) = children_pos {
                                    prev_stmt.position = prev_stmt.position.map(|(s, e)| (s.min(start), e.max(end))).or(Some((start, end)));
                                }
                            }
                            continue;
                        }
                        let (children, _) = self.extract_list_children(child, source)?;
                        statements.extend(children);
                        previous_index = None;
                    }
                    Node::Heading(_) => {
                        let content_position = self.get_clean_content_position(child, source);
                        let full_position = child.position().map(|p| (p.start.offset, p.end.offset));
                        let (children, children_pos) = self.extract_direct_children(child, source)?;
                        statements.push(DokeStatement {
                            children,
                            content_position,
                            position: full_position,
                            children_position: children_pos,
                        });
                        previous_index = None;
                    }
                    _ => {
                        let child_statements = self.process_other_node(child, source)?;
                        statements.extend(child_statements);
                        previous_index = None;
                    }
                }
            }
        }
        Ok(statements)
    }

    fn merge_positions(
        &self,
        pos1: Option<(usize, usize)>,
        pos2: Option<(usize, usize)>,
    ) -> Option<(usize, usize)> {
        match (pos1, pos2) {
            (Some((s1, e1)), Some((s2, e2))) => Some((s1.min(s2), e1.max(e2))),
            (Some(pos), None) | (None, Some(pos)) => Some(pos),
            (None, None) => None,
        }
    }

    fn extract_list_children(
        &self,
        list_node: &Node,
        source: &str,
    ) -> Result<(Vec<DokeStatement>, Option<(usize, usize)>), DokeParseError> {
        let mut children = Vec::new();
        let mut children_start: Option<usize> = None;
        let mut children_end: Option<usize> = None;
        if let Node::List(list) = list_node {
            for item in &list.children {
                let content_position = self.get_clean_content_position(item, source);
                let (item_children, item_children_pos) = self.extract_list_item_children(item, source)?;
                if let Some(pos) = item.position() {
                    children_start = Some(children_start.map_or(pos.start.offset, |s| s.min(pos.start.offset)));
                    children_end = Some(children_end.map_or(pos.end.offset, |e| e.max(pos.end.offset)));
                }
                children.push(DokeStatement {
                    children: item_children,
                    content_position,
                    position: item.position().map(|p| (p.start.offset, p.end.offset)),
                    children_position: item_children_pos,
                });
            }
        }
        let children_position = if let (Some(s), Some(e)) = (children_start, children_end) { Some((s, e)) } else { None };
        Ok((children, children_position))
    }

    fn extract_list_item_children(
        &self,
        list_item: &Node,
        source: &str,
    ) -> Result<(Vec<DokeStatement>, Option<(usize, usize)>), DokeParseError> {
        let mut children = Vec::new();
        let mut children_start: Option<usize> = None;
        let mut children_end: Option<usize> = None;
        if let Some(child_nodes) = list_item.children() {
            for child in child_nodes {
                match child {
                    Node::Paragraph(_) => {}
                    Node::List(_) => {
                        let (nested, nested_pos) = self.extract_list_children(child, source)?;
                        if let Some((start, end)) = nested_pos {
                            children_start = Some(children_start.map_or(start, |s| s.min(start)));
                            children_end = Some(children_end.map_or(end, |e| e.max(end)));
                        }
                        children.extend(nested);
                    }
                    _ => {
                        let other = self.process_other_node(child, source)?;
                        if let Some(pos) = child.position() {
                            children_start = Some(children_start.map_or(pos.start.offset, |s| s.min(pos.start.offset)));
                            children_end = Some(children_end.map_or(pos.end.offset, |e| e.max(pos.end.offset)));
                        }
                        children.extend(other);
                    }
                }
            }
        }
        let children_position = if let (Some(s), Some(e)) = (children_start, children_end) { Some((s, e)) } else { None };
        Ok((children, children_position))
    }

    fn extract_direct_children(
        &self,
        parent: &Node,
        source: &str,
    ) -> Result<(Vec<DokeStatement>, Option<(usize, usize)>), DokeParseError> {
        let mut children = Vec::new();
        let mut start: Option<usize> = None;
        let mut end: Option<usize> = None;
        if let Some(child_nodes) = parent.children() {
            for child in child_nodes {
                let child_stmts = self.process_other_node(child, source)?;
                children.extend(child_stmts);
                if let Some(pos) = child.position() {
                    start = Some(start.map_or(pos.start.offset, |s| s.min(pos.start.offset)));
                    end = Some(end.map_or(pos.end.offset, |e| e.max(pos.end.offset)));
                }
            }
        }
        let children_position = if let (Some(s), Some(e)) = (start, end) { Some((s, e)) } else { None };
        Ok((children, children_position))
    }

    fn process_other_node(
        &self,
        node: &Node,
        _source: &str,
    ) -> Result<Vec<DokeStatement>, DokeParseError> {
        match node {
            Node::Heading(_) => {
                let content_position = self.get_clean_content_position(node, _source);
                let full_position = node.position().map(|p| (p.start.offset, p.end.offset));
                let (children, children_pos) = self.extract_direct_children(node, _source)?;
                Ok(vec![DokeStatement {
                    children,
                    content_position,
                    position: full_position,
                    children_position: children_pos,
                }])
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Computes the range of inline content for paragraphs, headings, or list items.
    /// Includes Text, inline Code, Links, Emphasis, and Wikilinks.
    fn get_clean_content_position(&self, node: &Node, _source: &str) -> Option<(usize, usize)> {
        match node {
            Node::ListItem(_) => {
                let mut start = usize::MAX;
                let mut end = 0;
                if let Some(children) = node.children() {
                    for child in children {
                        match child {
                            Node::Paragraph(_) => {
                                if let Some((s, e)) = self.get_clean_content_position(child, _source) {
                                    start = start.min(s);
                                    end = end.max(e);
                                }
                            }
                            Node::List(_) => {} // block list, excluded
                            _ => {}
                        }
                    }
                }
                if start <= end { Some((start, end)) } else { None }
            }
            Node::Paragraph(_) | Node::Heading(_) => {
                let mut start = usize::MAX;
                let mut end = 0;
                if let Some(children) = node.children() {
                    for child in children {
                        match child {
                            Node::Text(_) | Node::Code(_) | Node::Link(_) => {
                                if let Some(pos) = child.position() {
                                    start = start.min(pos.start.offset);
                                    end = end.max(pos.end.offset);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if start <= end { Some((start, end)) } else { None }
            }
            Node::Text(_) | Node::Code(_) | Node::Link(_) => {
                let pos = node.position()?;
                Some((pos.start.offset, pos.end.offset))
            }
            _ => None,
        }
    }
}
