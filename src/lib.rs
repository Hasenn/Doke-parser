#![allow(dead_code)]
mod base_parser;
pub mod parsers;
pub mod semantic;
pub mod utility;
use crate::base_parser::Position;
use crate::semantic::{DokeNodeState, DokeValidate, DokeValidationError};
use base_parser::{DokeBaseParser, DokeStatement};
use markdown::ParseOptions;
pub use semantic::GodotValue;
pub use semantic::{DokeNode, DokeOut, DokeParser, Hypo};
use std::collections::HashMap;

#[derive(Debug)]
/// Normalized DokeDocument returned from the pipeline
pub struct DokeDocument {
    pub nodes: Vec<DokeNode>,
    pub frontmatter: HashMap<String, GodotValue>,
}

/// A pipe of semantic parsers.
/// using validate() or run_markdown() on an input will parse it with the pipe.
///
/// The pipe automatically translates the input markdown into `DokeNode`,
/// a semantic and mutable tree of statements.
#[derive(Debug)]
pub struct DokePipe<'a> {
    parsers: Vec<Box<dyn DokeParser + 'a>>,
    parse_options: ParseOptions,
}

impl<'a> DokePipe<'a> {
    pub fn new() -> Self {
        Self {
            parsers: vec![],
            parse_options: ParseOptions::default(),
        }
    }
    /// Validates the tree to try and produce a value
    /// ```
    /// use doke::{DokePipe, GodotValue, parsers};
    ///
    /// let pipe = DokePipe::new()
    ///    .add(parsers::FrontmatterTemplateParser);
    /// let res = pipe.validate("some input");
    /// ```
    /// This visits the tree depth-first, collecting errors for unresolved or errored nodes.
    /// For resolved nodes, it will convert them to GodotValues
    /// The most confident hypothesis will be promoted.
    /// If this promotion leads to an error (meaning the most sure parser marked the node as wrong)
    /// then this will also error.
    /// Any valid value found for a child node or constituent will call use_children or use_constituents on the values
    /// of the parent nodes.
    ///
    /// This builds a single object from all the parsed nodes,
    /// or collects errors to display.
    pub fn validate(&self, input: &str) -> Result<Vec<GodotValue>, DokeValidationError> {
        let doc = self.run_markdown(input);

        // Run validator on parsed nodes
        let mut nodes = doc.nodes;
        DokeValidate::validate_tree(&mut nodes, &doc.frontmatter)
    }

    pub fn add<P>(mut self, parser: P) -> Self
    where
        P: DokeParser + 'a,
    {
        self.parsers.push(Box::new(parser));
        self
    }

    pub fn map<P>(mut self, parser: P) -> Self
    where
        P: DokeParser + 'a,
    {
        #[derive(Debug)]
        struct Mapper<P: DokeParser> {
            parser: P,
        }

        impl<P: DokeParser> DokeParser for Mapper<P> {
            fn process(&self, node: &mut DokeNode, frontmatter: &HashMap<String, GodotValue>) {
                self.parser.process(node, frontmatter);
                for child in &mut node.children {
                    self.process(child, frontmatter);
                }
            }
        }

        self.parsers.push(Box::new(Mapper { parser }));
        self
    }
    /// Adds a parser that conditionally processes nodes based on a filter predicate.
    ///
    /// This method creates a parser that recursively traverses the node tree and applies
    /// the given parser only to nodes for which the filter closure returns `true`.
    /// Child nodes are always traversed regardless of whether their parent was processed.
    ///
    /// # Parameters
    /// - `parser`: The parser to apply to matching nodes
    /// - `filter`: A closure that takes a reference to a node, frontmatter and depth, and returns
    ///   `true` if the parser should be applied to that node
    ///
    /// # Examples
    /// ```
    /// use doke::{DokePipe, GodotValue, parsers};
    /// let effect_parser = parsers::SentenceParser::from_yaml(r#"
    /// Effect:
    /// - "Does Stuff"
    /// "#).unwrap();
    /// let pipe = DokePipe::new()
    ///     .add(parsers::FrontmatterTemplateParser)
    ///     .filter_map(effect_parser, |_, _, dpth| dpth > 2 );
    /// ```
    ///
    /// # Note
    /// The filter is evaluated for each node during processing, and the parser is only
    /// applied to nodes where the filter returns `true`. All child nodes are still
    /// traversed to check if they match the filter condition.
    pub fn filter_map<P, F>(mut self, parser: P, filter: F) -> Self
    where
        P: DokeParser + 'a,
        F: Fn(&DokeNode, &HashMap<String, GodotValue>, usize) -> bool + 'a,
    {
        struct FilterMapper<
            P: DokeParser,
            F: Fn(&DokeNode, &HashMap<String, GodotValue>, usize) -> bool,
        > {
            parser: P,
            filter: F,
        }
        impl<P: DokeParser, F: Fn(&DokeNode, &HashMap<String, GodotValue>, usize) -> bool>
            std::fmt::Debug for FilterMapper<P, F>
        {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.parser.fmt(f)
            }
        }

        impl<P: DokeParser, F: Fn(&DokeNode, &HashMap<String, GodotValue>, usize) -> bool>
            DokeParser for FilterMapper<P, F>
        {
            fn process(&self, node: &mut DokeNode, frontmatter: &HashMap<String, GodotValue>) {
                self.process_with_level(node, frontmatter, 0);
            }
        }

        impl<P: DokeParser, F: Fn(&DokeNode, &HashMap<String, GodotValue>, usize) -> bool>
            FilterMapper<P, F>
        {
            fn process_with_level(
                &self,
                node: &mut DokeNode,
                frontmatter: &HashMap<String, GodotValue>,
                level: usize,
            ) {
                // Apply the filter to determine if we should run the parser on this node
                if (self.filter)(node, frontmatter, level) {
                    self.parser.process(node, frontmatter);
                }

                // Recursively process children with incremented level
                for child in &mut node.children {
                    self.process_with_level(child, frontmatter, level + 1);
                }
            }
        }

        self.parsers.push(Box::new(FilterMapper { parser, filter }));
        self
    }

    /// Run pipeline on a Markdown string and return a DokeDocument
    pub fn run_markdown(&self, input: &str) -> DokeDocument {
        // Extract frontmatter and remaining markdown
        let (frontmatter_str, markdown_str) = extract_frontmatter(input);

        // Convert markdown into MD AST using configured ParseOptions
        let root_node = markdown::to_mdast(&markdown_str, &self.parse_options).unwrap();

        let doc = DokeBaseParser::parse_document(&root_node, frontmatter_str).unwrap();

        // Convert frontmatter YAML → normalized HashMap<String, GodotValue>
        let mut fm_map = HashMap::new();
        if let Some(fm) = &doc.frontmatter {
            if let yaml_rust2::Yaml::Hash(h) = fm {
                for (k, v) in h {
                    if let yaml_rust2::Yaml::String(s) = k {
                        let key = normalize_key(s);
                        fm_map.insert(key, yaml_value_to_godot(v.clone()));
                    }
                }
            }
        }

        fn statements_to_nodes(stmts: &[DokeStatement], input: &str) -> Vec<DokeNode> {
            stmts
                .iter()
                .map(|stmt| {
                    let statement_position = stmt
                        .statement_position
                        .clone()
                        .unwrap_or(Position { start: 0, end: 0 });
                    let statement_text = if let Some(pos) = &stmt.statement_position {
                        // Safely slice the input string using byte offsets
                        input
                            .get(pos.start..pos.end)
                            .unwrap_or_default()
                            .to_string()
                    } else {
                        "".to_string()
                    };

                    DokeNode {
                        statement: statement_text,
                        state: DokeNodeState::Unresolved,
                        children: statements_to_nodes(&stmt.children, input),
                        parse_data: HashMap::new(),
                        constituents: HashMap::new(),
                        span: statement_position,
                    }
                })
                .collect()
        }

        let mut nodes = statements_to_nodes(&doc.statements, markdown_str);

        for parser in &self.parsers {
            for node in nodes.iter_mut() {
                parser.process(node, &fm_map);
            }
        }

        DokeDocument {
            nodes,
            frontmatter: fm_map,
        }
    }

    /// Optional: allow setting parse options in the future
    pub fn with_parse_options(mut self, opts: ParseOptions) -> Self {
        self.parse_options = opts;
        self
    }
}

/// Normalize frontmatter keys: lowercase + spaces → _
fn normalize_key(key: &str) -> String {
    key.trim().to_lowercase().replace(' ', "_")
}

/// Extract frontmatter from a markdown string.
/// Returns (Some(frontmatter_str), rest_of_markdown) if frontmatter exists.
fn extract_frontmatter(input: &str) -> (Option<&str>, &str) {
    let mut parts = input.splitn(3, "---");

    // First part is before the first '---' (likely empty if frontmatter at start)
    let _first = parts.next().unwrap_or("").trim_start();

    // Second part is frontmatter
    if let Some(fm) = parts.next() {
        // Third part is the rest of the markdown
        let rest = parts
            .next()
            .unwrap_or("")
            .trim_start_matches(|c| c == '\r' || c == '\n');
        return (Some(fm.trim()), rest);
    }

    // No frontmatter found
    (None, input)
}

/// Convert yaml_rust2::Yaml → GodotValue
fn yaml_value_to_godot(y: yaml_rust2::Yaml) -> GodotValue {
    match y {
        yaml_rust2::Yaml::String(s) => GodotValue::String(s),
        yaml_rust2::Yaml::Integer(i) => GodotValue::Int(i),
        yaml_rust2::Yaml::Real(f) => GodotValue::Float(f.parse().unwrap_or(0.0)),
        yaml_rust2::Yaml::Boolean(b) => GodotValue::Bool(b),
        yaml_rust2::Yaml::Array(a) => {
            GodotValue::Array(a.into_iter().map(yaml_value_to_godot).collect())
        }
        yaml_rust2::Yaml::Hash(h) => {
            let mut map = HashMap::new();
            for (k, v) in h {
                if let yaml_rust2::Yaml::String(s) = k {
                    map.insert(normalize_key(&s), yaml_value_to_godot(v));
                }
            }
            GodotValue::Dict(map)
        }
        _ => GodotValue::Nil,
    }
}
