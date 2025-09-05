// src/parsers/typed_sentences.rs
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use glob::glob;
use hashlink::LinkedHashMap;
use thiserror::Error;
use yaml_rust2::Yaml;

use crate::parsers::sentence::SentenceParser;
use crate::{DokeNode, DokeNodeState, DokeParser, GodotValue};

#[derive(Debug, Error)]
pub enum TypedSentencesError {
    #[error("YAML parse error: {0}")]
    YamlParseError(String),

    #[error("Invalid rule configuration: {0}")]
    InvalidRule(String),

    #[error("No matching sentence parser for node")]
    NoMatchingParser,

    #[error("File error: {0}")]
    FileError(String),

    #[error("Glob pattern error: {0}")]
    GlobError(String),
}

#[derive(Debug, Clone)]
pub struct ParserReference {
    pub pattern: String,
    pub base_dir: PathBuf,
}

// src/parsers/typed_sentences.rs
#[derive(Debug, Clone)]
pub enum ChildSpec {
    Simple(Vec<String>), // Old syntax: children: [ItemEffect, DamageEffect]
    Structured(HashMap<String, Vec<String>>), // New syntax: children: {damage_effects: [DamageEffect], other_effects: [ItemEffect]}
}

impl ChildSpec {
    fn allowed(&self, child_abstract_type: &str) -> bool {
        match self {
            ChildSpec::Simple(items) => items.contains(&child_abstract_type.to_string()),
            ChildSpec::Structured(hash_map) => hash_map
                .values()
                .any(|child_types| child_types.contains(&child_abstract_type.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TypeRule {
    pub target_type: String,
    pub parser_ref: ParserReference,
    pub priority: i32,
    pub children: ChildSpec, // Changed from allowed_children
    pub sentence_parser: SentenceParser,
}

#[derive(Debug)]
pub struct TypedSentencesParser {
    rules: Vec<TypeRule>,
}

impl TypedSentencesParser {
    pub fn from_config_file(config_path: &Path) -> Result<Self, TypedSentencesError> {
        let config_content = fs::read_to_string(config_path)
            .map_err(|e| TypedSentencesError::FileError(e.to_string()))?;

        let base_dir = config_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        Self::from_config(&config_content, &base_dir)
    }

    pub fn from_config(config: &str, base_dir: &Path) -> Result<Self, TypedSentencesError> {
        let docs = yaml_rust2::YamlLoader::load_from_str(config)
            .map_err(|e| TypedSentencesError::YamlParseError(e.to_string()))?;

        let doc = docs
            .first()
            .ok_or(TypedSentencesError::YamlParseError("Empty YAML".into()))?;

        let mut rules = Vec::new();

        if let Yaml::Hash(root) = doc {
            if let Some(Yaml::Array(rules_array)) = root.get(&Yaml::String("rules".into())) {
                for rule_config in rules_array {
                    if let Yaml::Hash(rule_hash) = rule_config {
                        let rule = Self::parse_rule(rule_hash, base_dir)?;
                        rules.push(rule);
                    }
                }
            }
        }

        // Load the actual sentence parsers from the referenced files
        let mut loaded_rules = Vec::new();
        for rule in rules {
            let sentence_parser =
                Self::load_parser_from_reference(&rule.parser_ref, rule.target_type.clone())?;

            loaded_rules.push(TypeRule {
                sentence_parser,
                target_type: rule.target_type.clone(),
                priority: rule.priority,
                children: ChildSpec::Simple(vec![]),
                parser_ref: rule.parser_ref,
            });
        }

        // Sort by priority (highest first)
        loaded_rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(Self {
            rules: loaded_rules,
        })
    }

    fn parse_rule(
        rule_hash: &LinkedHashMap<Yaml, Yaml>,
        base_dir: &Path,
    ) -> Result<TypeRule, TypedSentencesError> {
        let mut target_type = None;
        let mut parser_pattern = None;
        let mut priority = 0;
        let mut children = ChildSpec::Simple(Vec::new());

        for (key, value) in rule_hash {
            if let Yaml::String(key_str) = key {
                match key_str.as_str() {
                    "for" => {
                        if let Yaml::String(type_str) = value {
                            target_type = Some(type_str.clone());
                        }
                    }
                    "parser" => {
                        if let Yaml::String(pattern) = value {
                            parser_pattern = Some(pattern.clone());
                        }
                    }
                    "priority" => {
                        if let Yaml::Integer(prio) = value {
                            priority = *prio as i32;
                        }
                    }
                    "children" => {
                        if let Ok(spec) = Self::parse_child_spec(value) {
                            children = spec
                        }
                    }
                    _ => {}
                }
            }
        }

        let target_type = target_type.ok_or(TypedSentencesError::InvalidRule(
            "Missing 'for' field".into(),
        ))?;
        let parser_pattern = parser_pattern.ok_or(TypedSentencesError::InvalidRule(
            "Missing 'parser' field".into(),
        ))?;

        Ok(TypeRule {
            target_type: target_type.clone(),
            parser_ref: ParserReference {
                pattern: parser_pattern,
                base_dir: base_dir.to_path_buf(),
            },
            priority,
            children,
            sentence_parser: SentenceParser {
                phrases: Vec::new(),
                type_patterns: HashMap::new(),
                abstract_type: "".into(),
                children_map: HashMap::new(),
            }, // Temporary placeholder
        })
    }

    fn parse_child_spec(yaml: &Yaml) -> Result<ChildSpec, TypedSentencesError> {
        match yaml {
            // Old syntax: children: [ItemEffect, DamageEffect]
            Yaml::Array(children_array) => {
                let mut child_types = Vec::new();
                for child in children_array {
                    if let Yaml::String(child_type) = child {
                        child_types.push(child_type.clone());
                    }
                }
                Ok(ChildSpec::Simple(child_types))
            }
            // New syntax: children: {damage_effects: [DamageEffect], other_effects: [ItemEffect]}
            Yaml::Hash(children_map) => {
                let mut structured_children = HashMap::new();
                for (field_name, child_types) in children_map {
                    if let Yaml::String(field_str) = field_name {
                        if let Yaml::Array(types_array) = child_types {
                            let mut types_vec = Vec::new();
                            for child_type in types_array {
                                if let Yaml::String(type_str) = child_type {
                                    types_vec.push(type_str.clone());
                                }
                            }
                            structured_children.insert(field_str.clone(), types_vec);
                        }
                    }
                }
                Ok(ChildSpec::Structured(structured_children))
            }
            _ => Ok(ChildSpec::Simple(Vec::new())), // Empty if invalid
        }
    }
    fn load_parser_from_reference(
        parser_ref: &ParserReference,
        abstract_type: String,
    ) -> Result<SentenceParser, TypedSentencesError> {
        let mut config_content = String::new();
        let mut found_files = Vec::new();

        let full_pattern = parser_ref
            .base_dir
            .join(&parser_ref.pattern)
            .to_string_lossy()
            .into_owned();

        let glob_iter = glob(&full_pattern).map_err(|e| {
            TypedSentencesError::GlobError(format!(
                "Invalid glob pattern '{}': {}",
                full_pattern, e
            ))
        })?;

        for entry in glob_iter {
            match entry {
                Ok(path) => {
                    if path.is_file() && is_dokedef_file(&path) {
                        match fs::read_to_string(&path) {
                            Ok(content) => {
                                config_content.push_str(&content);
                                config_content.push_str("\n---\n");
                                found_files.push(path);
                            }
                            Err(e) => {
                                println!("Warning: Could not read file {}: {}", path.display(), e);
                            }
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "Warning: Error accessing file in pattern {}: {}",
                        full_pattern, e
                    );
                }
            }
        }

        if found_files.is_empty() {
            return Err(TypedSentencesError::FileError(format!(
                "No .dokedef.yaml files found for pattern: {} (searched: {})",
                parser_ref.pattern, full_pattern
            )));
        }

        println!(
            "Loaded parser from {} files: {:?}",
            found_files.len(),
            found_files
        );

        SentenceParser::from_yaml(abstract_type, &config_content).map_err(|e| {
            TypedSentencesError::InvalidRule(format!(
                "Failed to parse YAML from {} files: {}",
                found_files.len(),
                e
            ))
        })
    }

    fn rule_matches_parent(&self, rule: &TypeRule, parent_abstract_type: Option<&str>) -> bool {
        parent_abstract_type.map_or(true, |parent_type| {
            let child_spec = &rule.children;
            child_spec.allowed(parent_type)
        })
    }

    fn try_process_with_rule(
        &self,
        node: &mut DokeNode,
        frontmatter: &HashMap<String, GodotValue>,
        rule: &TypeRule,
    ) -> bool {
        // Store original state manually (simplified approach)
        let was_unresolved = matches!(node.state, DokeNodeState::Unresolved);

        rule.sentence_parser.process(node, frontmatter);

        if let DokeNodeState::Resolved(_) = &node.state {
            node.parse_data.insert(
                "abstract_type".to_string(),
                GodotValue::String(rule.target_type.clone()),
            );
            true
        } else {
            // If we didn't resolve it, restore the unresolved state
            if was_unresolved {
                node.state = DokeNodeState::Unresolved;
            }
            false
        }
    }

    fn process_node_recursive(
        &self,
        node: &mut DokeNode,
        frontmatter: &HashMap<String, GodotValue>,
        parent_abstract_type: Option<&str>,
        depth: usize,
    ) {
        if depth > 100 {
            return;
        }

        if let DokeNodeState::Unresolved = &node.state {
            let mut candidate_rules: Vec<&TypeRule> = self
                .rules
                .iter()
                .filter(|rule| self.rule_matches_parent(rule, parent_abstract_type))
                .collect();

            candidate_rules.sort_by(|a, b| b.priority.cmp(&a.priority));

            for rule in candidate_rules {
                if self.try_process_with_rule(node, frontmatter, rule) {
                    break;
                }
            }

            if let DokeNodeState::Unresolved = &node.state {
                let mut all_rules: Vec<&TypeRule> = self.rules.iter().collect();
                all_rules.sort_by(|a, b| b.priority.cmp(&a.priority));

                for rule in all_rules {
                    if self.try_process_with_rule(node, frontmatter, rule) {
                        break;
                    }
                }
            }
        }

        let current_abstract_type = if let DokeNodeState::Resolved(_) = &node.state {
            node.parse_data.get("abstract_type").and_then(|v| {
                if let GodotValue::String(s) = v {
                    Some(s.as_str())
                } else {
                    None
                }
            })
        } else {
            None
        };

        for child in &mut node.children {
            self.process_node_recursive(child, frontmatter, current_abstract_type, depth + 1);
        }

        for constituent in node.constituents.values_mut() {
            self.process_node_recursive(constituent, frontmatter, current_abstract_type, depth + 1);
        }
    }

    pub fn debug_glob_pattern(
        &self,
        pattern: &str,
        base_dir: &Path,
    ) -> Result<Vec<PathBuf>, TypedSentencesError> {
        let full_pattern = base_dir.join(pattern).to_string_lossy().into_owned();
        let mut results = Vec::new();

        for entry in
            glob(&full_pattern).map_err(|e| TypedSentencesError::GlobError(e.to_string()))?
        {
            match entry {
                Ok(path) => results.push(path),
                Err(e) => println!("Warning: {}", e),
            }
        }

        Ok(results)
    }
}

impl DokeParser for TypedSentencesParser {
    fn process(&self, node: &mut DokeNode, frontmatter: &HashMap<String, GodotValue>) {
        self.process_node_recursive(node, frontmatter, None, 0);
    }
}

fn is_dokedef_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        if ext != "yaml" && ext != "yml" {
            return false;
        }
    } else {
        return false;
    }

    if let Some(name) = path.file_stem() {
        let name_str = name.to_string_lossy();
        name_str.contains("dokedef") || name_str.contains("doke") || name_str.ends_with("Parser")
    } else {
        false
    }
}
