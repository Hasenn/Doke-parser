use std::collections::HashMap;
use std::error::Error;
use regex::Regex;
use thiserror::Error;
use yaml_rust2::{Yaml, YamlLoader};

use crate::{
    GodotValue, DokeParser, DokeNode, DokeNodeState, Hypo, DokeOut,
};

// ----------------- Configuration -----------------

const MAX_RECURSION_DEPTH: usize = 100;

// ----------------- Configuration Structures -----------------

#[derive(Debug, Clone)]
pub struct PhraseConfig {
    pub pattern: String,
    pub regex: Regex,
    pub output_type: String,
    pub parameters: Vec<ParameterDefinition>,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParameterDefinition {
    pub name: String,
    pub param_type: String,
}

#[derive(Debug, Clone)]
pub struct TypePattern {
    pub pattern: String,
    pub regex: Regex,
    pub enum_value: String,
}

// ----------------- Sentence Parser -----------------

#[derive(Debug)]
pub struct SentenceParser {
    phrases: Vec<PhraseConfig>,
    enum_values: HashMap<String, GodotValue>,
    type_patterns: HashMap<String, Vec<TypePattern>>,
    phrases_by_type: HashMap<String, Vec<PhraseConfig>>,
}

impl SentenceParser {
    pub fn from_yaml(config: &str) -> Result<Self, SentenceParserError> {
        let docs = YamlLoader::load_from_str(config)
            .map_err(|e| SentenceParserError::YamlParseError(e.to_string()))?;
        
        let doc = docs.first().ok_or(SentenceParserError::EmptyYaml)?;
        
        let mut phrases = Vec::new();
        let mut enum_values = HashMap::new();
        let mut type_patterns = HashMap::new();
        let mut phrases_by_type = HashMap::new();
        
        // Parse enum values
        if let Some(enum_section) = doc["enum_values"].as_vec() {
            for enum_item in enum_section {
                if let Yaml::Hash(enum_map) = enum_item {
                    for (enum_name, enum_val) in enum_map {
                        if let (Yaml::String(name), Yaml::Integer(value)) = (enum_name, enum_val) {
                            enum_values.insert(name.clone(), GodotValue::Int(*value));
                        }
                    }
                }
            }
        }
        
        // Parse type patterns (like Target) and phrase patterns
        for (key, value) in doc.as_hash().unwrap() {
            if let Yaml::String(section_name) = key {
                if section_name == "enum_values" {
                    continue;
                }
                
                if let Yaml::Array(items) = value {
                    // Check if this is a type pattern section (like Target)
                    let is_type_pattern_section = items.iter().any(|item| match item {
                        Yaml::Hash(_) => true,
                        _ => false,
                    });
                    
                    if is_type_pattern_section {
                        // This is a type pattern section (e.g., Target)
                        let mut type_pattern_list = Vec::new();
                        
                        for pattern_item in items {
                            if let Yaml::Hash(pattern_map) = pattern_item {
                                for (pattern_text, enum_val_name) in pattern_map {
                                    if let (Yaml::String(pattern_str), Yaml::String(val_name)) = (pattern_text, enum_val_name) {
                                        let regex = Regex::new(&format!("^{}$", regex::escape(&pattern_str)))
                                            .map_err(|e| SentenceParserError::RegexError(pattern_str.clone(), e.to_string()))?;
                                        
                                        type_pattern_list.push(TypePattern {
                                            pattern: pattern_str.clone(),
                                            regex,
                                            enum_value: val_name.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        
                        type_patterns.insert(section_name.clone(), type_pattern_list);
                    } else {
                        // This is a phrase pattern section (e.g., DamageEffect, ReactionEffect)
                        let mut type_phrases = Vec::new();
                        
                        for pattern in items {
                            if let Yaml::String(pattern_str) = pattern {
                                let phrase_config = Self::parse_phrase_pattern(&pattern_str, section_name)?;
                                phrases.push(phrase_config.clone());
                                type_phrases.push(phrase_config);
                            }
                        }
                        
                        phrases_by_type.insert(section_name.clone(), type_phrases);
                    }
                }
            }
        }
        
        Ok(Self {
            phrases,
            enum_values,
            type_patterns,
            phrases_by_type,
        })
    }
    
    fn parse_phrase_pattern(pattern: &str, output_type: &str) -> Result<PhraseConfig, SentenceParserError> {
        let parameters = Self::extract_parameters(pattern);
        let regex_pattern = Self::build_regex_pattern(pattern, &parameters)?;
        let regex = Regex::new(&regex_pattern)
            .map_err(|e| SentenceParserError::RegexError(pattern.to_string(), e.to_string()))?;
        
        Ok(PhraseConfig {
            pattern: pattern.to_string(),
            regex,
            output_type: output_type.to_string(),
            parameters,
            context: None,
        })
    }
    
    fn extract_parameters(pattern: &str) -> Vec<ParameterDefinition> {
        let mut params = Vec::new();
        let re = Regex::new(r"\{([^}:]+)(?::([^}]+))?\}").unwrap();
        
        for cap in re.captures_iter(pattern) {
            let name = cap[1].trim().to_string();
            let param_type = cap.get(2)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_else(|| "string".to_string());
            
            params.push(ParameterDefinition { name, param_type });
        }
        
        params
    }
    
    fn build_regex_pattern(pattern: &str, parameters: &[ParameterDefinition]) -> Result<String, SentenceParserError> {
        let mut regex_pattern = String::new();
        regex_pattern.push('^');
        
        let re = Regex::new(r"\{([^}:]+)(?::([^}]+))?\}").unwrap();
        let mut last_end = 0;
        
        for (i, cap) in re.captures_iter(pattern).enumerate() {
            let m = cap.get(0).unwrap();
            
            // Add text before parameter
            if m.start() > last_end {
                let text = &pattern[last_end..m.start()];
                regex_pattern.push_str(&regex::escape(text));
            }
            
            // Add parameter capture group with specific patterns based on type
            let param_def = &parameters[i];
            let capture_group = match param_def.param_type.to_lowercase().as_str() {
                "int" => r"([-+]?(?:0[bB][01]+|0[oO][0-7]+|0[xX][0-9a-fA-F]+|\d+))", // binary, octal, hex, decimal
                "float" => r"([-+]?(?:\d+\.\d*|\.\d+)(?:[eE][-+]?\d+)?)", // float with scientific notation
                "bool" => r"(true|false|yes|no|1|0)", // boolean values
                _ => r"(.+?)", // default: capture anything (non-greedy)
            };
            
            regex_pattern.push_str(capture_group);
            last_end = m.end();
        }
        
        // Add remaining text
        if last_end < pattern.len() {
            let text = &pattern[last_end..];
            regex_pattern.push_str(&regex::escape(text));
        }
        
        regex_pattern.push('$');
        Ok(regex_pattern)
    }
    
    fn match_phrase_exact(
        &self,
        statement: &str,
        phrase: &PhraseConfig,
    ) -> Result<HashMap<String, String>, SentenceParseError> {
        let captures = phrase.regex.captures(statement)
            .ok_or(SentenceParseError::NoMatch(phrase.pattern.clone()))?;
        
        let mut params = HashMap::new();
        
        for (i, param_def) in phrase.parameters.iter().enumerate() {
            if let Some(capture) = captures.get(i + 1) {
                let value_str = capture.as_str().trim().to_string();
                params.insert(param_def.name.clone(), value_str);
            }
        }
        
        Ok(params)
    }
    
    fn parse_parameter_value(&self, value: &str, param_type: &str) -> Result<GodotValue, SentenceParseError> {
        // Handle basic types with specific parsing
        match param_type.to_lowercase().as_str() {
            "int" => Self::parse_int(value)
                .map(GodotValue::Int)
                .map_err(|e| SentenceParseError::TypeParseError(param_type.to_string(), value.to_string(), e)),
            "float" => value.parse::<f64>()
                .map(GodotValue::Float)
                .map_err(|e| SentenceParseError::TypeParseError(param_type.to_string(), value.to_string(), e.to_string())),
            "string" => Ok(GodotValue::String(value.to_string())),
            "bool" => Self::parse_bool(value)
                .map(GodotValue::Bool)
                .map_err(|e| SentenceParseError::TypeParseError(param_type.to_string(), value.to_string(), e)),
            _ => {
                // Handle custom types with patterns (like Target)
                if let Some(patterns) = self.type_patterns.get(param_type) {
                    for type_pattern in patterns {
                        // Trim both the pattern and value for comparison
                        let trimmed_pattern = type_pattern.pattern.trim();
                        let trimmed_value = value.trim();
                        
                        if type_pattern.regex.is_match(trimmed_value) {
                            if let Some(enum_value) = self.enum_values.get(&type_pattern.enum_value) {
                                return Ok(enum_value.clone());
                            }
                            return Err(SentenceParseError::UnknownEnumValue(
                                param_type.to_string(),
                                type_pattern.enum_value.clone(),
                            ));
                        }
                    }
                }
                
                // If this is a complex type, return as string for recursive parsing
                Ok(GodotValue::String(value.to_string()))
            }
        }
    }
    
    fn parse_int(value: &str) -> Result<i64, String> {
        if value.starts_with("0b") || value.starts_with("0B") {
            i64::from_str_radix(&value[2..], 2)
                .map_err(|e| e.to_string())
        } else if value.starts_with("0o") || value.starts_with("0O") {
            i64::from_str_radix(&value[2..], 8)
                .map_err(|e| e.to_string())
        } else if value.starts_with("0x") || value.starts_with("0X") {
            i64::from_str_radix(&value[2..], 16)
                .map_err(|e| e.to_string())
        } else {
            value.parse::<i64>()
                .map_err(|e| e.to_string())
        }
    }
    
    fn parse_bool(value: &str) -> Result<bool, String> {
        match value.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(true),
            "false" | "no" | "0" => Ok(false),
            _ => Err(format!("Invalid boolean value: {}", value)),
        }
    }
    
    fn create_constituent_node(&self, value: &str, param_type: &str) -> DokeNode {
        DokeNode {
            statement: value.to_string(),
            state: DokeNodeState::Unresolved,
            children: Vec::new(),
            parse_data: HashMap::new(),
            constituents: HashMap::new(),
        }
    }
    
    fn is_basic_type(param_type: &str) -> bool {
        matches!(
            param_type.to_lowercase().as_str(),
            "int" | "float" | "bool" | "string"
        )
    }
    
    fn process_with_depth(&self, node: &mut DokeNode, depth: usize) {
        if depth > MAX_RECURSION_DEPTH {
            // Set error state instead of storing in parse_data
            node.state = DokeNodeState::Error(Box::new(SentenceParseError::MaxRecursionDepthExceeded));
            return;
        }
        
        if !matches!(node.state, DokeNodeState::Unresolved) {
            return;
        }
        
        let statement = node.statement.trim();
        let mut potential_matches = Vec::new();
        let mut parsing_errors = Vec::new();
        
        // Get the expected type from parse_data for optimized lookup
        let expected_type = node.parse_data.get("sentence_type")
            .and_then(|v| {
                if let GodotValue::String(s) = v {
                    Some(s.as_str())
                } else {
                    None
                }
            });
        
        // Use phrases for the expected type if available, otherwise check all phrases
        let phrases_to_check = if let Some(type_name) = expected_type {
            self.phrases_by_type.get(type_name).unwrap_or(&self.phrases)
        } else {
            &self.phrases
        };
        
        for phrase in phrases_to_check {
            match self.match_phrase_exact(statement, phrase) {
                Ok(raw_params) => {
                    potential_matches.push((phrase, raw_params));
                }
                Err(error) => {
                    parsing_errors.push(error);
                }
            }
        }
        
        if let Some((best_phrase, raw_params)) = potential_matches.into_iter().next() {
            let mut parsed_params = HashMap::new();
            let mut constituent_nodes = HashMap::new();
            
            // Parse parameters and create constituent nodes for complex types
            for param_def in &best_phrase.parameters {
                if let Some(raw_value) = raw_params.get(&param_def.name) {
                    if Self::is_basic_type(&param_def.param_type) {
                        // Parse basic types immediately
                        match self.parse_parameter_value(raw_value, &param_def.param_type) {
                            Ok(value) => {
                                parsed_params.insert(param_def.name.clone(), value);
                            }
                            Err(error) => {
                                // Store error in parse_data for debugging
                                node.parse_data.insert(
                                    format!("{}_error", param_def.name),
                                    GodotValue::String(error.to_string()),
                                );
                            }
                        }
                    } else {
                        // For complex types, create constituent nodes for recursive parsing
                        let mut constituent_node = self.create_constituent_node(raw_value, &param_def.param_type);
                        
                        // Set the expected type on the constituent node for optimized parsing
                        constituent_node.parse_data.insert(
                            "sentence_type".to_string(),
                            GodotValue::String(param_def.param_type.clone()),
                        );
                        
                        // Recursively process the constituent node
                        self.process_with_depth(&mut constituent_node, depth + 1);
                        
                        constituent_nodes.insert(param_def.name.clone(), constituent_node);
                    }
                }
            }
            
            // Store constituent nodes
            node.constituents.extend(constituent_nodes);
            
            let result = SentenceResult {
                output_type: best_phrase.output_type.clone(),
                parameters: parsed_params,
            };
            
            node.state = DokeNodeState::Resolved(Box::new(result));
        } else if !parsing_errors.is_empty() {
            let mut hypotheses = Vec::new();
            for error in parsing_errors {
                hypotheses.push(Box::new(ErrorHypo {
                    error,
                    statement: node.statement.clone(),
                }) as Box<dyn Hypo>);
            }
            node.state = DokeNodeState::Hypothesis(hypotheses);
        }
    }
}

// ----------------- DokeParser Implementation -----------------

impl DokeParser for SentenceParser {
    fn process(&self, node: &mut DokeNode, _frontmatter: &HashMap<String, GodotValue>) {
        self.process_with_depth(node, 0);
    }
}

// ----------------- Result Implementation -----------------

#[derive(Debug)]
struct SentenceResult {
    output_type: String,
    parameters: HashMap<String, GodotValue>,
}

impl DokeOut for SentenceResult {
    fn kind(&self) -> &'static str {
        "SentenceResult"
    }
    
    fn to_godot(&self) -> GodotValue {
        GodotValue::Resource {
            type_name: self.output_type.clone(),
            fields: self.parameters.clone(),
        }
    }
    
    fn use_child(&mut self, child: GodotValue) -> Result<(), Box<dyn Error>> {
        todo!()
    }
    
    fn use_constituent(&mut self, name: &str, value: GodotValue) -> Result<(), Box<dyn Error>> {
        self.parameters.insert(name.to_string(), value);
        Ok(())
    }
}

// ----------------- Error Types -----------------

#[derive(Debug, Error)]
pub enum SentenceParserError {
    #[error("YAML parse error: {0}")]
    YamlParseError(String),
    
    #[error("Empty YAML document")]
    EmptyYaml,
    
    #[error("Regex error for pattern '{0}': {1}")]
    RegexError(String, String),
    
    #[error("Invalid phrase pattern: {0}")]
    InvalidPattern(String),
}

#[derive(Debug, Error)]
pub enum SentenceParseError {
    #[error("Pattern '{0}' did not match statement")]
    NoMatch(String),
    
    #[error("Failed to parse parameters for pattern '{0}': {1:?}")]
    ParameterParseError(String, Vec<(String, String, String, String)>),
    
    #[error("Type parse error for {0} value '{1}': {2}")]
    TypeParseError(String, String, String),
    
    #[error("Unknown enum value '{1}' for type '{0}'")]
    UnknownEnumValue(String, String),
    
    #[error("Unknown type '{0}' for value '{1}'")]
    UnknownType(String, String),
    
    #[error("Maximum recursion depth exceeded")]
    MaxRecursionDepthExceeded,
}

// ----------------- Error Hypothesis -----------------

#[derive(Debug)]
struct ErrorHypo {
    error: SentenceParseError,
    statement: String,
}

impl Hypo for ErrorHypo {
    fn kind(&self) -> &'static str {
        "SentenceParseError"
    }
    
    fn confidence(&self) -> f32 {
        -1.0
    }
    
    fn promote(self: Box<Self>) -> Result<Box<dyn DokeOut>, Box<dyn Error>> {
        Err(Box::new(self.error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DokeNode, DokeNodeState, GodotValue};

    const TEST_YAML: &str = r#"
DamageEffect:
  - "Deals {damage: int}"
  - "Deals {damage: int} {target: Target}"

ReactionEffect:
  - "When hit: {damage_effect: DamageEffect}"

Target:
  - "to Enemies": TARGET_ENEMY
  - "to Allies": TARGET_ALLIED
  - "": TARGET_ANY

enum_values:
  - TARGET_ENEMY: 1
  - TARGET_ALLIED: 2
  - TARGET_ANY: 3
"#;

    #[test]
    fn test_parser_creation() {
        let parser = SentenceParser::from_yaml(TEST_YAML);
        assert!(parser.is_ok(), "Parser should be created successfully");
    }

    #[test]
    fn test_basic_damage_effect() {
        let parser = SentenceParser::from_yaml(TEST_YAML).unwrap();
        let mut node = DokeNode {
            statement: "Deals 10".to_string(),
            state: DokeNodeState::Unresolved,
            children: Vec::new(),
            parse_data: HashMap::new(),
            constituents: HashMap::new(),
        };

        parser.process(&mut node, &HashMap::new());

        if let DokeNodeState::Resolved(result) = &node.state {
            let out = result.to_godot();
            if let GodotValue::Resource { type_name, fields } = out {
                assert_eq!(type_name, "DamageEffect");
                assert_eq!(fields.get("damage"), Some(&GodotValue::Int(10)));
            } else {
                panic!("Expected Resource GodotValue");
            }
        } else {
            panic!("Node should be resolved");
        }
    }

    #[test]
    fn test_parameter_extraction() {
        let params = SentenceParser::extract_parameters("Deals {damage: int} to {target: Target}");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "damage");
        assert_eq!(params[0].param_type, "int");
        assert_eq!(params[1].name, "target");
        assert_eq!(params[1].param_type, "Target");
    }

    #[test]
    fn test_regex_pattern_building() {
        let params = vec![
            ParameterDefinition { name: "damage".to_string(), param_type: "int".to_string() },
            ParameterDefinition { name: "target".to_string(), param_type: "Target".to_string() },
        ];
        
        let regex = SentenceParser::build_regex_pattern("Deals {damage: int} to {target: Target}", &params).unwrap();
        // Should create a regex that matches the pattern with parameter capture groups
        assert!(regex.contains(r"([-+]?(?:0[bB][01]+|0[oO][0-7]+|0[xX][0-9a-fA-F]+|\d+))")); // int pattern
        assert!(regex.contains(r"(.+?)")); // default pattern
    }

    #[test]
    fn test_parse_int_various_formats() {
        assert_eq!(SentenceParser::parse_int("42"), Ok(42));
        assert_eq!(SentenceParser::parse_int("0b1010"), Ok(10)); // binary
        assert_eq!(SentenceParser::parse_int("0o755"), Ok(493)); // octal
        assert_eq!(SentenceParser::parse_int("0xFF"), Ok(255)); // hex
        assert_eq!(SentenceParser::parse_int("-123"), Ok(-123)); // negative
    }

    #[test]
    fn test_parse_bool_various_formats() {
        assert_eq!(SentenceParser::parse_bool("true"), Ok(true));
        assert_eq!(SentenceParser::parse_bool("yes"), Ok(true));
        assert_eq!(SentenceParser::parse_bool("1"), Ok(true));
        assert_eq!(SentenceParser::parse_bool("false"), Ok(false));
        assert_eq!(SentenceParser::parse_bool("no"), Ok(false));
        assert_eq!(SentenceParser::parse_bool("0"), Ok(false));
        assert!(SentenceParser::parse_bool("maybe").is_err());
    }

    #[test]
    fn test_unknown_pattern() {
        let parser = SentenceParser::from_yaml(TEST_YAML).unwrap();
        let mut node = DokeNode {
            statement: "This is not a valid pattern".to_string(),
            state: DokeNodeState::Unresolved,
            children: Vec::new(),
            parse_data: HashMap::new(),
            constituents: HashMap::new(),
        };

        parser.process(&mut node, &HashMap::new());

        // Should be in hypothesis state with errors
        if let DokeNodeState::Hypothesis(hypotheses) = &node.state {
            assert!(!hypotheses.is_empty());
        } else {
            panic!("Node should be in hypothesis state for unknown patterns");
        }
    }
}