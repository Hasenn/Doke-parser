use markdown::{mdast::Node, ParseOptions};
use std::fmt;

#[derive(Debug, Clone)]
pub struct DokeDocument {
    pub statements: Vec<DokeStatement>,
}

#[derive(Debug, Clone)]
pub struct DokeStatement {
    pub content: String,
    pub children: Vec<DokeStatement>,
    pub content_position: Option<(usize, usize)>, // Position of the content text only
    pub children_position: Option<(usize, usize)>, // Position of the children in source
}

#[derive(Debug, thiserror::Error)]
pub enum DokeParseError {
    #[error("Markdown parsing error: {0}")]
    MarkdownParseError(String),
    
    #[error("Invalid document structure")]
    InvalidStructure,
}

impl fmt::Display for DokeStatement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.content)?;
        if !self.children.is_empty() {
            write!(f, " ({} children)", self.children.len())?;
        }
        Ok(())
    }
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
        let root_node = markdown::to_mdast(input, &self.options)
            .map_err(|e| DokeParseError::MarkdownParseError(e.to_string()))?;

        let statements = self.extract_statements_with_hierarchy(&root_node, input)?;
        
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

            'children: for child in &root.children {
                match child {
                    // Paragraph → statement (can accept children only if it ends with :)
                    Node::Paragraph(_) => {
                        let content = self.extract_clean_text_content(child, source);
                        let content_position = self.get_clean_content_position(child, source);

                        let statement = DokeStatement {
                            content: content.clone(),
                            children: Vec::new(),
                            content_position,
                            children_position: None,
                        };

                        statements.push(statement);

                        // Only paragraphs ending with ':' can have children
                        if content.trim_end().ends_with(':') {
                            previous_index = Some(statements.len() - 1);
                        } else {
                            previous_index = None;
                        }
                    }

                    // List → attach only if last paragraph ended with ':'
                    Node::List(_) => {
                        if let Some(prev_idx) = previous_index {
                            let (children, children_pos) = self.extract_list_children(child, source)?;
                            if let Some(prev_stmt) = statements.get_mut(prev_idx) {
                                prev_stmt.children.extend(children);
                                prev_stmt.children_position =
                                    self.merge_positions(prev_stmt.children_position, children_pos);
                            }
                            continue 'children;
                        }

                        // Otherwise → treat as top-level list
                        let (children, _children_pos) = self.extract_list_children(child, source)?;
                        statements.extend(children);
                        previous_index = None;
                    }

                    // Headings → always new statement with potential children
                    Node::Heading(_) => {
                        let content = self.extract_clean_text_content(child, source);
                        let content_position = self.get_clean_content_position(child, source);
                        let (children, children_pos) = self.extract_direct_children(child, source)?;

                        statements.push(DokeStatement {
                            content,
                            children,
                            content_position,
                            children_position: children_pos,
                        });
                        previous_index = None;
                    }

                    // Fallback handler for other node types
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

    fn merge_positions(&self, pos1: Option<(usize, usize)>, pos2: Option<(usize, usize)>) -> Option<(usize, usize)> {
        match (pos1, pos2) {
            (Some((start1, end1)), Some((start2, end2))) => Some((start1.min(start2), end1.max(end2))),
            (Some(pos), None) => Some(pos),
            (None, Some(pos)) => Some(pos),
            (None, None) => None,
        }
    }

    fn extract_list_children(
        &self,
        list_node: &Node,
        source: &str,
    ) -> Result<(Vec<DokeStatement>, Option<(usize, usize)>), DokeParseError> {
        let mut children = Vec::new();
        let mut children_start = None;
        let mut children_end = None;
        
        if let Node::List(list) = list_node {
            for item in &list.children {
                let content = self.extract_clean_text_content(item, source);
                let (item_children, item_children_pos) = self.extract_list_item_children(item, source)?;
                let content_position = self.get_clean_content_position(item, source);

                if let Some(pos) = content_position {
                    children_start = children_start.min(Some(pos.0)).or(Some(pos.0));
                    children_end = children_end.max(Some(pos.1)).or(Some(pos.1));
                }

                children.push(DokeStatement {
                    content,
                    children: item_children,
                    content_position,
                    children_position: item_children_pos,
                });
            }
        }

        let children_position = if let (Some(start), Some(end)) = (children_start, children_end) {
            Some((start, end))
        } else {
            None
        };

        Ok((children, children_position))
    }

    fn extract_list_item_children(
        &self,
        list_item: &Node,
        source: &str,
    ) -> Result<(Vec<DokeStatement>, Option<(usize, usize)>), DokeParseError> {
        let mut children = Vec::new();
        let mut children_start = None;
        let mut children_end = None;

        if let Some(child_nodes) = list_item.children() {
            for child in child_nodes {
                match child {
                    Node::Paragraph(_) => {} // Paragraphs inside list item become content, not children
                    Node::List(_) => {
                        let nested_items = self.extract_list_children(child, source)?;
                        if !nested_items.0.is_empty() {
                            if let Some(pos) = child.position() {
                                children_start = children_start.min(Some(pos.start.offset)).or(Some(pos.start.offset));
                                children_end = children_end.max(Some(pos.end.offset)).or(Some(pos.end.offset));
                            }
                        }
                        children.extend(nested_items.0);
                    }
                    _ => {
                        let child_statements = self.process_other_node(child, source)?;
                        if !child_statements.is_empty() {
                            if let Some(pos) = child.position() {
                                children_start = children_start.min(Some(pos.start.offset)).or(Some(pos.start.offset));
                                children_end = children_end.max(Some(pos.end.offset)).or(Some(pos.end.offset));
                            }
                        }
                        children.extend(child_statements);
                    }
                }
            }
        }

        let children_position = if let (Some(start), Some(end)) = (children_start, children_end) {
            Some((start, end))
        } else {
            None
        };
        Ok((children, children_position))
    }

    fn extract_direct_children(
        &self,
        parent: &Node,
        source: &str,
    ) -> Result<(Vec<DokeStatement>, Option<(usize, usize)>), DokeParseError> {
        let mut children = Vec::new();
        let mut children_start = None;
        let mut children_end = None;
        
        if let Some(child_nodes) = parent.children() {
            for child in child_nodes {
                match child {
                    Node::Paragraph(_) | Node::Heading(_) | Node::List(_) => {
                        let child_statements = self.process_other_node(child, source)?;
                        if !child_statements.is_empty() {
                            if let Some(pos) = child.position() {
                                children_start = children_start.min(Some(pos.start.offset)).or(Some(pos.start.offset));
                                children_end = children_end.max(Some(pos.end.offset)).or(Some(pos.end.offset));
                            }
                        }
                        children.extend(child_statements);
                    }
                    _ => {}
                }
            }
        }

        let children_position = if let (Some(start), Some(end)) = (children_start, children_end) {
            Some((start, end))
        } else {
            None
        };
        
        Ok((children, children_position))
    }

    fn process_other_node(
        &self,
        node: &Node,
        source: &str,
    ) -> Result<Vec<DokeStatement>, DokeParseError> {
        match node {
            Node::Heading(_) => {
                let content = self.extract_clean_text_content(node, source);
                let (children, children_pos) = self.extract_direct_children(node, source)?;
                let content_position = self.get_clean_content_position(node, source);
                Ok(vec![DokeStatement {
                    content,
                    children,
                    content_position,
                    children_position: children_pos,
                }])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn extract_clean_text_content(&self, node: &Node, source: &str) -> String {
        match node {
            Node::Text(text) => text.value.clone(),
            Node::Paragraph(_) | Node::Heading(_) | Node::ListItem(_) => {
                self.collect_clean_text_from_children(node, source)
            }
            _ => String::new(),
        }
    }

    fn collect_clean_text_from_children(&self, node: &Node, source: &str) -> String {
        let mut content = String::new();

        if let Some(children) = node.children() {
            for child in children {
                match child {
                    Node::Text(text) => {
                        if !content.is_empty() { content.push(' '); }
                        content.push_str(&text.value);
                    }
                    Node::Code(code) => {
                        if !content.is_empty() { content.push(' '); }
                        content.push_str(&code.value);
                    }
                    Node::Link(link) => {
                        let mut parts = Vec::new();
                        for c in &link.children {
                            let t = self.collect_clean_text_from_children(c, source);
                            if !t.is_empty() { parts.push(t); }
                        }
                        if !parts.is_empty() {
                            if !content.is_empty() { content.push(' '); }
                            content.push_str(&parts.join(" "));
                        }
                    }
                    _ => {
                        let child_content = self.collect_clean_text_from_children(child, source);
                        if !child_content.is_empty() {
                            if !content.is_empty() { content.push(' '); }
                            content.push_str(&child_content);
                        }
                    }
                }
            }
        }

        content
    }

    fn get_clean_content_position(&self, node: &Node, source: &str) -> Option<(usize, usize)> {
        self.trim_node_content_position(node, source)
    }   
    fn trim_node_content_position(&self, node: &Node, source: &str) -> Option<(usize, usize)> {
        let pos = node.position()?;
        let mut start = pos.start.offset;
        let mut end = pos.end.offset;

        // Trim leading whitespace
        while start < end {
            let c = source[start..].chars().next()?;
            if !c.is_whitespace() { break; }
            start += c.len_utf8();
        }

        // Trim trailing whitespace
        while end > start {
            let c = source[..end].chars().rev().next()?;
            if !c.is_whitespace() { break; }
            end -= c.len_utf8();
        }

        // If this is a list item, skip bullet/number
        if let Node::ListItem(_) = node {
            // Get line text
            if let Some(line_end_rel) = source[start..end].find('\n') {
                let line = &source[start..start + line_end_rel];
                let re = regex::Regex::new(r"^(\s*[-*+] |\s*\d+\.\s+)").unwrap();
                if let Some(mat) = re.find(line) {
                    start += mat.end();
                }
            } else {
                // Single line
                let line = &source[start..end];
                let re = regex::Regex::new(r"^(\s*[-*+] |\s*\d+\.\s+)").unwrap();
                if let Some(mat) = re.find(line) {
                    start += mat.end();
                }
            }
        }

        if start < end { Some((start, end)) } else { None }
    }

}

impl DokeDocument {
    pub fn find_statements_by_content(&self, search: &str) -> Vec<&DokeStatement> {
        let mut results = Vec::new();
        self.find_statements_recursive(&self.statements, search, &mut results);
        results
    }

    fn find_statements_recursive<'a>(
        &'a self,
        statements: &'a [DokeStatement],
        search: &str,
        results: &mut Vec<&'a DokeStatement>,
    ) {
        for statement in statements {
            if statement.content.contains(search) {
                results.push(statement);
            }
            self.find_statements_recursive(&statement.children, search, results);
        }
    }

    fn trim_node_content_position(&self, node: &Node, source: &str) -> Option<(usize, usize)> {
        let pos = node.position()?;
        let mut start = pos.start.offset;
        let mut end = pos.end.offset;

        // Trim leading whitespace
        while start < end && source[start..].chars().next().unwrap().is_whitespace() {
            start += source[start..].chars().next().unwrap().len_utf8();
        }

        // Trim trailing whitespace
        while end > start && source[..end].chars().rev().next().unwrap().is_whitespace() {
            end -= source[..end].chars().rev().next().unwrap().len_utf8();
        }

        // If this is a list item, skip the bullet/number
        if let Node::ListItem(_) = node {
            if let Some(line_end) = source[start..end].find('\n') {
                let line = &source[start..start+line_end];
                let re = regex::Regex::new(r"^(\s*[-*+] |\s*\d+\.\s+)").unwrap();
                if let Some(mat) = re.find(line) {
                    start += mat.end();
                }
            }
        }

        if start < end { Some((start, end)) } else { None }
    }
}

