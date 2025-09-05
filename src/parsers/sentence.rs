// sentence_parser.rs
//
// Sentence parser supporting yaml-rust2 configuration,
// enum sections, return-specs (Type, Literal, Format),
// strict-case matching, whitespace-robust literals,
// phrase specificity, and recursive constituent parsing.

use polib::po_file::POParseError;
use regex::Regex;
use std::path::PathBuf;
use std::{collections::HashMap};

use yaml_rust2::{Yaml, YamlLoader};
use thiserror::Error;
use crate::base_parser::Position;
use crate::utility::update_po_file;
use crate::{DokeNode, DokeNodeState, DokeOut, DokeParser, GodotValue, Hypo};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_value<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
fn camel_to_const_case(input: &str) -> String {
    let mut result = String::new();
    let mut chars = input.chars().peekable();
    let mut prev_was_upper = false;
    let mut prev_was_lower = false;
    
    while let Some(c) = chars.next() {
        let is_upper = c.is_uppercase();
        
        // Add underscore if:
        // 1. Current char is uppercase AND previous was lowercase (camelCase boundary)
        // 2. Current char is lowercase AND previous was uppercase AND next is uppercase (aBc -> A_BC)
        if !result.is_empty() {
            if is_upper && prev_was_lower {
                result.push('_');
            } else if let Some(&next) = chars.peek() {
                if !is_upper && prev_was_upper && next.is_uppercase() {
                    result.push('_');
                }
            }
        }
        
        result.push(c.to_ascii_uppercase());
        
        prev_was_upper = is_upper;
        prev_was_lower = !is_upper;
    }
    
    result
}

const BASE32_ALPHABET: [char; 32] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
    '2', '3', '4', '5', '6', '7',
];

fn u64_to_base32(mut num: u64) -> String {
    if num == 0 {
        return "A".to_string();
    }
    
    let mut result = String::new();
    
    while num > 0 {
        let remainder = (num % 32) as usize;
        result.push(BASE32_ALPHABET[remainder]);
        num /= 32;
    }
    
    result.chars().rev().collect()
}

type Result<T> = std::result::Result<T, SentenceParseError>;

#[derive(Debug, Error)]
pub enum SentenceParseError {
    #[error("YAML parse error: {0}")]
    YamlParseError(String),

    #[error("Empty YAML document")]
    EmptyYaml,

    #[error("Regex error for pattern '{0}': {1}")]
    RegexError(String, String),

    #[error("Invalid pattern: {0}")]
    InvalidPattern(String),
    #[error("\"{0}\" : No sentence match")]
    NoMatch(String),
    #[error("Max recursion depth exceeded : {0}")]
    MaxRecursionDepthExceeded(String),
    #[error("Could not read translation file : {0}")]
    TranslationWriteError(#[from] POParseError)
}



// ----------------- Config structures -----------------

#[derive(Debug, Clone)]
pub struct ParameterDefinition {
    pub name: String,
    pub param_type: String,
}

#[derive(Debug, Clone)]
pub enum ReturnSpec {
    Type(String),
    Literal(GodotValue),
    Format(String),
}

#[derive(Debug, Clone)]
pub struct PhraseConfig {
    pub pattern: String,
    pub regex: Regex,
    pub parameters: Vec<ParameterDefinition>,
    pub return_spec: ReturnSpec,
    pub section: String,
}

impl PhraseConfig {
    // A traduction key, Deterministic in the phrase pattern.
    // Currently uses the section name the rule was in and a hash of the rule string
    fn make_tr_key(&self)-> String {
        let hash : String = u64_to_base32(hash_value(&self.pattern)).chars().take(7).collect();
        format!("{}_{}", camel_to_const_case(&self.section), hash)
    }
}


#[derive(Debug)]
pub struct SentenceParser {
    phrases: Vec<PhraseConfig>,
    type_patterns: HashMap<String, Vec<(Regex, GodotValue)>>,
}

// ----------------- Parser construction -----------------

impl SentenceParser {
    pub fn get_en_translation(&self) -> HashMap<String, String> {
        let mut trads = HashMap::new();
        let re = Regex::new(r"\{([^}:]+)(?:\s*:\s*[^}]*)?\}").unwrap();
        
        for phrase in &self.phrases {
            let cleaned_pattern = re.replace_all(&phrase.pattern, "{$1}");
            trads.insert(phrase.make_tr_key(), cleaned_pattern.to_string());
        }
        trads
    }

    pub fn make_or_update_po_file(&self,path : PathBuf, project_id_version : String) -> Result<()> {
        update_po_file(&path, self.get_en_translation(), project_id_version)?;
        Ok(())
    }

    pub fn from_yaml(config: &str) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let docs = YamlLoader::load_from_str(config)?;
        let doc = docs.first().ok_or("Empty YAML")?;
        let mut phrases = Vec::new();
        let type_patterns = HashMap::new();

        let top_hash = doc.as_hash().ok_or("Top-level YAML must be a mapping")?;
        let param_re = Regex::new(r"\{([^}:]+)(?::([^}]+))?\}")?;

        for (k, v) in top_hash {
            let section_name = match k {
                Yaml::String(s) => s.clone(),
                _ => continue,
            };

            if let Some(items) = v.as_vec() {
                for item in items {
                    match item {
                        Yaml::String(phrase_str) => {
                            let (regex, params) = build_regex_for_phrase(phrase_str, &param_re)?;
                            phrases.push(PhraseConfig {
                                pattern: phrase_str.clone(),
                                regex,
                                parameters: params,
                                return_spec: ReturnSpec::Type(section_name.clone()),
                                section: section_name.clone(),
                            });
                        }
                        Yaml::Hash(map) => {
                            for (mk, mv) in map {
                                let phrase_text =
                                    mk.as_str().ok_or("Phrase key must be string")?.to_string();
                                let return_spec = parse_rhs_to_return_spec(mv, &section_name)?;
                                let (regex, params) =
                                    build_regex_for_phrase(&phrase_text, &param_re)?;
                                phrases.push(PhraseConfig {
                                    pattern: phrase_text,
                                    regex,
                                    parameters: params,
                                    return_spec,
                                    section: section_name.clone(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(Self {
            phrases,
            type_patterns,
        })
    }
}

// ----------------- Processing -----------------

impl SentenceParser {
    pub fn process_with_depth(
        &self,
        node: &mut DokeNode,
        frontmatter: &HashMap<String, GodotValue>,
        depth: usize,
    ) {
        if depth > 100 {
            node.state = DokeNodeState::Error(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Max recursion",
            )));
            return;
        }

        if !matches!(node.state, DokeNodeState::Unresolved) {
            return;
        }
        // trim whitespace and trailing .
        let statement = node.statement.trim().trim_end_matches(|c| ".:".contains(c));
        let phrases_to_check: Vec<&PhraseConfig> = self.phrases.iter().collect();
        let mut matches: Vec<(&PhraseConfig, HashMap<String, String>)> = Vec::new();

        for phrase in phrases_to_check {
            if let Ok(raw) = match_phrase_exact(statement, phrase) {
                matches.push((phrase, raw));
            }
        }

        if matches.is_empty() {
            node.state = DokeNodeState::Hypothesis(vec![Box::new(ErrorHypo {
                error: crate::parsers::sentence::SentenceParseError::NoMatch(statement.to_string()),
                statement: statement.to_string(),
            })]);
            return;
        }

        matches.sort_by_key(|(p, _)| phrase_specificity(p));
        let (best_phrase, raw_params) = matches.pop().unwrap();
        let (parsed_params, constituent_nodes) =
            self.parse_parameters(&best_phrase.parameters, &raw_params, frontmatter, &node.span);

        // attach constituents
        node.constituents.extend(constituent_nodes);
        let tr_key : String = best_phrase.make_tr_key();
        let result = match &best_phrase.return_spec {
            ReturnSpec::Type(t) => SentenceResult::new_type(t.clone(), parsed_params, tr_key),
            ReturnSpec::Literal(lv) => SentenceResult::new_literal(lv.clone(), parsed_params, tr_key),
            ReturnSpec::Format(fmt) => {
                let final_str = perform_format_string(fmt, &parsed_params, frontmatter);
                SentenceResult::new_literal(GodotValue::String(final_str), parsed_params, tr_key)
            }
        };

        node.state = DokeNodeState::Resolved(Box::new(result));
    }

    fn parse_parameters(
        &self,
        param_defs: &[ParameterDefinition],
        raw_params: &HashMap<String, String>,
        frontmatter: &HashMap<String, GodotValue>,
        span : &Position
    ) -> (HashMap<String, GodotValue>, HashMap<String, DokeNode>) {
        let mut parsed_params = HashMap::new();
        let mut constituent_nodes = HashMap::new();

        for param_def in param_defs {
            match raw_params.get(&param_def.name) {
                Some(raw_val) => {
                    if is_basic_type(&param_def.param_type) {
                        if let Ok(v) = parse_basic_parameter(raw_val, &param_def.param_type) {
                            parsed_params.insert(param_def.name.clone(), v);
                        }
                    } else {
                        let mut child = create_constituent_node(raw_val, &param_def.param_type, span);
                        child.parse_data.insert(
                            "sentence_type".to_string(),
                            GodotValue::String(param_def.param_type.clone()),
                        );
                        self.process_with_depth(&mut child, frontmatter, 0);
                        constituent_nodes.insert(param_def.name.clone(), child);
                    }
                }
                None => {
                }
            }
        }

        (parsed_params, constituent_nodes)
    }
}

// DokeParser trait
impl DokeParser for SentenceParser {
    fn process(&self, node: &mut DokeNode, frontmatter: &HashMap<String, GodotValue>) {
        self.process_with_depth(node, frontmatter, 0);
    }
}

// ----------------- Helpers -----------------

fn yaml_to_godot_value(y: &Yaml) -> GodotValue {
    match y {
        Yaml::String(s) => GodotValue::String(s.clone()),
        Yaml::Integer(i) => GodotValue::Int(*i),
        Yaml::Real(r) => {
            // yaml_rust2 stores reals as strings like "3.14"
            r.parse::<f64>()
                .map(GodotValue::Float)
                .unwrap_or(GodotValue::Float(0.0))
        }
        Yaml::Boolean(b) => GodotValue::Bool(*b),
        Yaml::Array(arr) => GodotValue::Array(arr.iter().map(yaml_to_godot_value).collect()),
        Yaml::Hash(h) => {
            let mut map = HashMap::new();
            for (k, v) in h {
                if let Yaml::String(s) = k {
                    map.insert(s.clone(), yaml_to_godot_value(v));
                }
            }
            GodotValue::Dict(map)
        }
        _ => GodotValue::Nil,
    }
}

fn is_basic_type(param_type: &str) -> bool {
    matches!(
        param_type.to_lowercase().as_str(),
        "int" | "float" | "bool" | "string"
    )
}

fn parse_basic_parameter(value: &str, param_type: &str) -> std::result::Result<GodotValue, String> {
    match param_type.to_lowercase().as_str() {
        "int" => {
            // support hex/octal/binary prefixes
            if value.starts_with("0b") || value.starts_with("0B") {
                i64::from_str_radix(&value[2..], 2)
                    .map(GodotValue::Int)
                    .map_err(|e| e.to_string())
            } else if value.starts_with("0o") || value.starts_with("0O") {
                i64::from_str_radix(&value[2..], 8)
                    .map(GodotValue::Int)
                    .map_err(|e| e.to_string())
            } else if value.starts_with("0x") || value.starts_with("0X") {
                i64::from_str_radix(&value[2..], 16)
                    .map(GodotValue::Int)
                    .map_err(|e| e.to_string())
            } else {
                value
                    .parse::<i64>()
                    .map(GodotValue::Int)
                    .map_err(|e| e.to_string())
            }
        }
        "float" => value
            .parse::<f64>()
            .map(GodotValue::Float)
            .map_err(|e| e.to_string()),
        "bool" => match value.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(GodotValue::Bool(true)),
            "false" | "no" | "0" => Ok(GodotValue::Bool(false)),
            _ => Err(format!("Invalid boolean value: {}", value)),
        },
        "string" => Ok(GodotValue::String(value.to_string())),
        _ => Err(format!("Unknown basic type: {}", param_type)),
    }
}

fn create_constituent_node(value: &str, _param_type: &str, span : &Position) -> DokeNode {
    DokeNode {
        statement: value.to_string(),
        state: DokeNodeState::Unresolved,
        children: Vec::new(),
        parse_data: HashMap::new(),
        constituents: HashMap::new(),
        span : span.clone()
    }
}

fn perform_format_string(
    fmt: &str,
    params: &HashMap<String, GodotValue>,
    front: &HashMap<String, GodotValue>,
) -> String {
    // replace occurrences of {name} with:
    //  1) params[name] if present
    //  2) front[name] if present
    //  3) keep {name} as-is otherwise
    let re = Regex::new(r"\{([^}]+)\}").unwrap();
    let mut out = String::new();
    let mut last = 0;
    for cap in re.captures_iter(fmt) {
        let m = cap.get(0).unwrap();
        let key = cap.get(1).unwrap().as_str();
        out.push_str(&fmt[last..m.start()]);
        if let Some(v) = params.get(key) {
            out.push_str(&godot_value_to_string(v));
        } else if let Some(v) = front.get(key) {
            out.push_str(&godot_value_to_string(v));
        } else {
            // keep placeholder as-is
            out.push_str(m.as_str());
        }
        last = m.end();
    }
    out.push_str(&fmt[last..]);
    out
}

fn godot_value_to_string(v: &GodotValue) -> String {
    match v {
        GodotValue::Nil => "".to_string(),
        GodotValue::Bool(b) => b.to_string(),
        GodotValue::Int(i) => i.to_string(),
        GodotValue::Float(f) => f.to_string(),
        GodotValue::String(s) => s.clone(),
        GodotValue::Array(a) => {
            let parts: Vec<String> = a.iter().map(|gv| godot_value_to_string(gv)).collect();
            format!("[{}]", parts.join(", "))
        }
        GodotValue::Dict(m) => {
            let parts: Vec<String> = m
                .iter()
                .map(|(k, gv)| format!("{}:{}", k, godot_value_to_string(gv)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        GodotValue::Resource { type_name, fields } => {
            let mut parts: Vec<String> = vec![format!("type={}", type_name)];
            for (k, v) in fields {
                parts.push(format!("{}={}", k, godot_value_to_string(v)));
            }
            format!("Resource({})", parts.join(","))
        }
    }
}

// Build a regex for a phrase pattern, turning literal whitespace into \s+,
// and capturing parameter groups according to their types.
fn build_regex_for_phrase(
    phrase: &str,
    param_re: &Regex,
) -> std::result::Result<(Regex, Vec<ParameterDefinition>), Box<dyn std::error::Error>> {
    let mut parameters: Vec<ParameterDefinition> = Vec::new();
    let mut regex_pattern = String::new();
    regex_pattern.push('^');

    let mut last_end = 0usize;

    for cap in param_re.captures_iter(phrase) {
        let m = cap.get(0).unwrap();
        // literal before parameter
        if m.start() > last_end {
            let text = &phrase[last_end..m.start()];
            push_literal(&mut regex_pattern, text);
        }

        let mut name = cap.get(1).unwrap().as_str().trim().to_string();
        let param_type = cap
            .get(2)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_else(|| "string".to_string());

        let optional = name.ends_with(":?");
        if optional {
            name = name[..name.len() - 2].to_string(); // remove :?
        }
        // add capture group by type
        let capture_group = match param_type.to_lowercase().as_str() {
            "int" => r"([-+]?(?:0[bB][01]+|0[oO][0-7]+|0[xX][0-9a-fA-F]+|\d+))".to_string(),
            "float" => r"([-+]?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?)".to_string(),
            "bool" => r"(true|false|yes|no|1|0)".to_string(),
            _ => r"(.+?)".to_string(), // non-greedy default
        };

        let group_regex = if optional {
            // whitespace + capture_group is optional
            format!(r"(?:\s+{})?", capture_group)
        } else {
            capture_group
        };

        regex_pattern.push_str(&group_regex);

        parameters.push(ParameterDefinition {
            name,
            param_type,
        });

        last_end = m.end();
    }

    // trailing literal
    if last_end < phrase.len() {
        let text = &phrase[last_end..];
        push_literal(&mut regex_pattern, text);
    }

    regex_pattern.push('$');

    let regex = Regex::new(&regex_pattern).map_err(|e| format!("{}", e))?;
    Ok((regex, parameters))
}

// Split trailing whitespace from a literal chunk.
// Returns (prefix_without_trailing_ws, had_trailing_ws)
fn split_trailing_ws(s: &str) -> (&str, bool) {
    let mut last_non_ws_byte = 0usize;
    let mut any_ws = false;
    for (idx, ch) in s.char_indices() {
        if !ch.is_whitespace() {
            last_non_ws_byte = idx + ch.len_utf8();
        } else {
            any_ws = true;
        }
    }
    if any_ws && last_non_ws_byte < s.len() {
        (&s[..last_non_ws_byte], true)
    } else {
        (s, false)
    }
}

// replace contiguous whitespace by \s+, escape other chars
fn push_literal(buf: &mut String, s: &str) {
    let mut in_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !in_space {
                buf.push_str(r"\s+");
                in_space = true;
            }
        } else {
            in_space = false;
            buf.push_str(&regex::escape(&ch.to_string()));
        }
    }
}

// match a phrase exactly using its compiled regex and return raw param strings
fn match_phrase_exact(
    statement: &str,
    phrase: &PhraseConfig,
) -> std::result::Result<HashMap<String, String>, SentenceParseError> {
    let caps = phrase
        .regex
        .captures(statement)
        .ok_or(SentenceParseError::NoMatch(phrase.pattern.clone()))?;
    let mut out: HashMap<String, String> = HashMap::new();
    for (i, param_def) in phrase.parameters.iter().enumerate() {
        if let Some(m) = caps.get(i + 1) {
            out.insert(param_def.name.clone(), m.as_str().trim().to_string());
        }
    }
    Ok(out)
}

// compute specificity: more literal chars and fewer params => higher specificity
fn phrase_specificity(p: &PhraseConfig) -> (usize, usize) {
    let mut literal = p.pattern.len();
    let mut params = 0usize;
    for pd in &p.parameters {
        params += 1;
        literal = literal.saturating_sub(pd.name.len() + pd.param_type.len() + 4);
    }
    (literal, usize::MAX - params)
}

// parse RHS yaml node into ReturnSpec
fn parse_rhs_to_return_spec(
    node: &Yaml,
    section_default: &str,
) -> std::result::Result<ReturnSpec, SentenceParseError> {
    match node {
        Yaml::Null => Ok(ReturnSpec::Type(section_default.to_string())),
        Yaml::String(s) => {
            let s_trim = s.trim();
            // l"..." literal string
            if let Some(inner) = s_trim
                .strip_prefix("l\"")
                .and_then(|r| r.strip_suffix('\"'))
            {
                return Ok(ReturnSpec::Literal(GodotValue::String(inner.to_string())));
            }
            // f"..." format string
            if let Some(inner) = s_trim
                .strip_prefix("f\"")
                .and_then(|r| r.strip_suffix('\"'))
            {
                return Ok(ReturnSpec::Format(inner.to_string()));
            }
            // plain scalar might be int/bool/float (literal), or a type name
            // try parse int
            if let Ok(i) = s_trim.parse::<i64>() {
                return Ok(ReturnSpec::Literal(GodotValue::Int(i)));
            }
            if let Ok(f) = s_trim.parse::<f64>() {
                return Ok(ReturnSpec::Literal(GodotValue::Float(f)));
            }
            if matches!(
                s_trim.to_lowercase().as_str(),
                "true" | "false" | "yes" | "no" | "1" | "0"
            ) {
                let b = matches!(s_trim.to_lowercase().as_str(), "true" | "yes" | "1");
                return Ok(ReturnSpec::Literal(GodotValue::Bool(b)));
            }
            // otherwise treat as Type name
            Ok(ReturnSpec::Type(s_trim.to_string()))
        }
        // if RHS is numeric/bool in YAML itself, parse directly
        Yaml::Integer(i) => Ok(ReturnSpec::Literal(GodotValue::Int(*i))),
        Yaml::Real(r) => {
            let f = r.parse::<f64>().unwrap_or(0.0);
            Ok(ReturnSpec::Literal(GodotValue::Float(f)))
        }
        Yaml::Boolean(b) => Ok(ReturnSpec::Literal(GodotValue::Bool(*b))),
        other => Err(SentenceParseError::InvalidPattern(format!(
            "Unsupported RHS: {:?}",
            other
        ))),
    }
}

// ----------------- SentenceResult + DokeOut implementation -----------------

#[derive(Debug)]
struct SentenceResult {
    output_type: String, // type name when resource
    parameters: HashMap<String, GodotValue>,
    literal_value: Option<GodotValue>, // when a phrase returns a literal value instead of a Resource
    tr_key: String
}

impl SentenceResult {
    fn new_type(t: String, params: HashMap<String, GodotValue>, tr_key : String) -> Self {
        Self {
            output_type: t,
            parameters: params,
            literal_value: None,
            tr_key
        }
    }
    fn new_literal(val: GodotValue, params: HashMap<String, GodotValue>, tr_key : String) -> Self {
        Self {
            output_type: "".to_string(),
            parameters: params,
            literal_value: Some(val),
            tr_key
        }
    }
}

impl DokeOut for SentenceResult {
    fn kind(&self) -> &'static str {
        "SentenceResult"
    }

    fn to_godot(&self) -> GodotValue {
        if let Some(lit) = &self.literal_value {
            lit.clone()
        } else {
            let mut fields = self.parameters.clone();
            fields.insert("doke_tr_key".into(), GodotValue::String(self.tr_key.clone()));
            GodotValue::Resource {
                type_name: self.output_type.clone(),
                fields,
            }
        }
    }

    fn use_child(&mut self, child: GodotValue) -> std::result::Result<(), Box<dyn std::error::Error>> {
        match self.parameters.entry("children".into()) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if let GodotValue::Array(a) = e.get_mut() {
                    a.push(child);
                    Ok(())
                } else {
                    Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "children field is not an array",
                    )))
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(GodotValue::Array(vec![child]));
                Ok(())
            }
        }
    }

    fn use_constituent(&mut self, name: &str, value: GodotValue) -> std::result::Result<(), Box<dyn std::error::Error>> {
        self.parameters.insert(name.to_string(), value);
        Ok(())
    }
}

// ----------------- Parsing error types & error hypo -----------------


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
    fn promote(self: Box<Self>) -> std::result::Result<Box<dyn DokeOut>, Box<dyn std::error::Error>> {
        Err(Box::new(self.error))
    }
}

// ----------------- Utility: parse RHS and substitution helpers -----------------

// (already defined above) perform_format_string & godot_value_to_string

// ----------------- End of file -----------------

#[cfg(test)]
mod tests {
    use crate::base_parser::Position;
    use crate::parsers::DebugPrinter;
    use crate::DokeParser;
    use crate::parsers::sentence::SentenceParser;
    use crate::semantic::{DokeNode, DokeNodeState, DokeValidate, GodotValue};
    use std::collections::HashMap;

    // Helper: process a statement and return the resolved GodotValue directly
    fn resolve_node(
        parser: &SentenceParser,
        statement: &str,
        frontmatter: HashMap<String, GodotValue>,
    ) -> GodotValue {
        let mut node = DokeNode {
            statement: statement.to_string(),
            state: DokeNodeState::Unresolved,
            children: Vec::new(),
            parse_data: HashMap::new(),
            constituents: HashMap::new(),
            span: Position{start: 0, end : 0}
        };

        // Parse into hypotheses / unresolved states
        parser.process(&mut node, &frontmatter);
        let debug_printer = DebugPrinter;
        debug_printer.process(&mut node, &frontmatter);
        // Run validator to resolve fully
        let mut roots = vec![node];
        let result =
            DokeValidate::validate_tree(&mut roots, &frontmatter).expect("Validation failed");

        // We only had one root node
        result.into_iter().next().unwrap()
    }

    // ✅ FIXED: now always passes frontmatter argument
    #[test]
    fn test_nested_reaction_effect() {
        let yaml = r#"
DamageEffect:
  - "Deals {damage: int}"

ReactionEffect:
  - "When hit: {damage_effect: DamageEffect}"
"#;
        let parser = SentenceParser::from_yaml(yaml).unwrap();
        let val = resolve_node(&parser, "When hit: Deals 7", HashMap::new());

        if let GodotValue::Resource { type_name, fields } = val {
            assert_eq!(type_name, "ReactionEffect");
            if let GodotValue::Resource {
                type_name: inner_type,
                fields: inner_fields,
            } = fields["damage_effect"].clone()
            {
                assert_eq!(inner_type, "DamageEffect");
                assert_eq!(inner_fields.get("damage"), Some(&GodotValue::Int(7)));
            } else {
                panic!("Expected inner DamageEffect resource");
            }
        } else {
            panic!("Expected Resource");
        }
    }

    // ✅ FIXED: now always passes frontmatter argument
    #[test]
    fn test_multiple_effects_in_one_vocab() {
        let yaml = r#"
DamageEffect:
  - "Deals {damage: int}"

HealEffect:
  - "Heals {amount: int}"

ComboEffect:
  - "First: {dmg: DamageEffect}, Then: {heal: HealEffect}"
"#;
        let parser = SentenceParser::from_yaml(yaml).unwrap();
        let val = resolve_node(&parser, "First: Deals 10, Then: Heals 5", HashMap::new());

        if let GodotValue::Resource { type_name, fields } = val {
            assert_eq!(type_name, "ComboEffect");
            if let GodotValue::Resource {
                type_name: dmg_type,
                fields: dmg_fields,
            } = fields["dmg"].clone()
            {
                assert_eq!(dmg_type, "DamageEffect");
                assert_eq!(dmg_fields.get("damage"), Some(&GodotValue::Int(10)));
            } else {
                panic!("Expected DamageEffect");
            }
            if let GodotValue::Resource {
                type_name: heal_type,
                fields: heal_fields,
            } = fields["heal"].clone()
            {
                assert_eq!(heal_type, "HealEffect");
                assert_eq!(heal_fields.get("amount"), Some(&GodotValue::Int(5)));
            } else {
                panic!("Expected HealEffect");
            }
        } else {
            panic!("Expected Resource");
        }
    }

    #[test]
    fn test_nested_combo_with_reaction() {
        let yaml = r#"
DamageEffect:
  - "Deals {damage: int}"

HealEffect:
  - "Heals {amount: int}"

ReactionEffect:
  - "When hit: {damage_effect: DamageEffect}"

ComboEffect:
  - "First: {dmg: DamageEffect}, Then: {heal: HealEffect}, Reaction: {reaction: ReactionEffect}"
"#;

        let parser = SentenceParser::from_yaml(yaml).unwrap();
        let val = resolve_node(
            &parser,
            "First: Deals 10, Then: Heals 5, Reaction: When hit: Deals 2",
            HashMap::new(),
        );

        if let GodotValue::Resource { type_name, fields } = val {
            assert_eq!(type_name, "ComboEffect");

            // dmg
            if let GodotValue::Resource {
                type_name: dmg_type,
                fields: dmg_fields,
            } = fields["dmg"].clone()
            {
                assert_eq!(dmg_type, "DamageEffect");
                assert_eq!(dmg_fields.get("damage"), Some(&GodotValue::Int(10)));
            } else {
                panic!("Expected DamageEffect");
            }

            // heal
            if let GodotValue::Resource {
                type_name: heal_type,
                fields: heal_fields,
            } = fields["heal"].clone()
            {
                assert_eq!(heal_type, "HealEffect");
                assert_eq!(heal_fields.get("amount"), Some(&GodotValue::Int(5)));
            } else {
                panic!("Expected HealEffect");
            }

            // reaction
            if let GodotValue::Resource {
                type_name: reaction_type,
                fields: reaction_fields,
            } = fields["reaction"].clone()
            {
                assert_eq!(reaction_type, "ReactionEffect");

                if let GodotValue::Resource {
                    type_name: inner_type,
                    fields: inner_fields,
                } = reaction_fields["damage_effect"].clone()
                {
                    assert_eq!(inner_type, "DamageEffect");
                    assert_eq!(inner_fields.get("damage"), Some(&GodotValue::Int(2)));
                } else {
                    panic!("Expected inner DamageEffect");
                }
            } else {
                panic!("Expected ReactionEffect");
            }
        } else {
            panic!("Expected ComboEffect");
        }
    }

    #[test]
    fn test_multi_level_reactions() {
        let yaml = r#"
DamageEffect:
  - "Deals {damage: int}"

ReactionEffect:
  - "On hit: {damage_effect: DamageEffect}"

SuperReactionEffect:
  - "Triggered: {reaction: ReactionEffect}"
"#;

        let parser = SentenceParser::from_yaml(yaml).unwrap();
        let val = resolve_node(&parser, "Triggered: On hit: Deals 7", HashMap::new());

        if let GodotValue::Resource { type_name, fields } = val {
            assert_eq!(type_name, "SuperReactionEffect");
            if let GodotValue::Resource {
                type_name: reaction_type,
                fields: reaction_fields,
            } = fields["reaction"].clone()
            {
                assert_eq!(reaction_type, "ReactionEffect");
                if let GodotValue::Resource {
                    type_name: inner_type,
                    fields: inner_fields,
                } = reaction_fields["damage_effect"].clone()
                {
                    assert_eq!(inner_type, "DamageEffect");
                    assert_eq!(inner_fields.get("damage"), Some(&GodotValue::Int(7)));
                } else {
                    panic!("Expected inner DamageEffect");
                }
            } else {
                panic!("Expected ReactionEffect");
            }
        } else {
            panic!("Expected SuperReactionEffect");
        }
    }
}
