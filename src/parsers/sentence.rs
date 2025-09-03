// sentence_parser.rs
//
// Sentence parser supporting yaml-rust2 configuration,
// enum sections, return-specs (Type, Literal, Format),
// strict-case matching, whitespace-robust literals,
// phrase specificity, and recursive constituent parsing.

use std::collections::HashMap;
use std::error::Error;

use regex::Regex;
use yaml_rust2::{Yaml, YamlLoader};

use crate::{DokeNode, DokeNodeState, DokeParser, DokeOut, GodotValue, Hypo};

const MAX_RECURSION_DEPTH: usize = 100;

// ----------------- Config data structures -----------------

#[derive(Debug, Clone)]
pub struct ParameterDefinition {
    pub name: String,
    pub param_type: String,
}

#[derive(Debug, Clone)]
pub enum ReturnSpec {
    Type(String),           // e.g., DamageEffect
    Literal(GodotValue),    // e.g., Int(1), String("..."), Bool(true)
    Format(String),         // e.g., f"Hello {who}"
}

#[derive(Debug, Clone)]
pub struct PhraseConfig {
    pub pattern: String,
    pub regex: Regex,
    pub parameters: Vec<ParameterDefinition>,
    pub return_spec: ReturnSpec,
    pub section: String, // section name (default Type if return_spec is Type)
}

#[derive(Debug)]
pub struct SentenceParser {
    phrases: Vec<PhraseConfig>,
    // For enums / type pattern sections:
    // map type_name -> vec of (regex, GodotValue)
    type_patterns: HashMap<String, Vec<(Regex, GodotValue)>>,
}

// ----------------- Errors -----------------

#[derive(Debug)]
pub enum SentenceParserError {
    YamlParseError(String),
    EmptyYaml,
    RegexError(String, String),
    InvalidPattern(String),
}

impl std::fmt::Display for SentenceParserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SentenceParserError::YamlParseError(s) => write!(f, "YAML parse error: {}", s),
            SentenceParserError::EmptyYaml => write!(f, "Empty YAML document"),
            SentenceParserError::RegexError(p, e) => {
                write!(f, "Regex error for pattern '{}': {}", p, e)
            }
            SentenceParserError::InvalidPattern(s) => write!(f, "Invalid pattern: {}", s),
        }
    }
}

impl Error for SentenceParserError {}

// ----------------- Implementation -----------------

impl SentenceParser {
    pub fn from_yaml(config: &str) -> Result<Self, SentenceParserError> {
        let docs = YamlLoader::load_from_str(config)
            .map_err(|e| SentenceParserError::YamlParseError(e.to_string()))?;
        let doc = docs.first().ok_or(SentenceParserError::EmptyYaml)?;

        let mut phrases: Vec<PhraseConfig> = Vec::new();
        let mut type_patterns: HashMap<String, Vec<(Regex, GodotValue)>> = HashMap::new();

        // top-level must be a hash/map
        let top_hash = match doc.as_hash() {
            Some(h) => h,
            None => {
                return Err(SentenceParserError::YamlParseError(
                    "Top-level YAML must be a mapping".to_string(),
                ))
            }
        };

        // parameter capture: {name[: type]?}
        let param_re = Regex::new(r"\{([^}:]+)(?::([^}]+))?\}").unwrap();

        for (k, v) in top_hash {
            // only consider string keys
            let section_name = match k {
                Yaml::String(s) => s.clone(),
                _ => continue,
            };

            // detect enum sections
            if section_name.starts_with("enum ") {
                let type_name = section_name["enum ".len()..].trim().to_string();
                // expect v to be an array of mappings
                if let Some(arr) = v.as_vec() {
                    let mut patterns_vec: Vec<(Regex, GodotValue)> = Vec::new();
                    for item in arr {
                        if let Yaml::Hash(map) = item {
                            for (mk, mv) in map {
                                // mk should be string (pattern)
                                let pat = match mk {
                                    Yaml::String(s) => s.clone(),
                                    _ => continue,
                                };
                                // parse mv into GodotValue
                                let val = yaml_to_godot_value(mv);
                                // compile regex: exact match of the pattern text
                                // allow empty string keys as catch-all
                                let regex = Regex::new(&format!("^{}$", regex::escape(&pat)))
                                    .map_err(|e| {
                                        SentenceParserError::RegexError(pat.clone(), e.to_string())
                                    })?;
                                patterns_vec.push((regex, val));
                            }
                        } else {
                            return Err(SentenceParserError::InvalidPattern(
                                format!("enum {} must contain mappings", type_name),
                            ));
                        }
                    }
                    type_patterns.insert(type_name, patterns_vec);
                } else {
                    return Err(SentenceParserError::InvalidPattern(
                        format!("enum {} must be a sequence", section_name),
                    ));
                }
                continue;
            }

            // otherwise, regular section: v is expected to be sequence
            if let Some(items) = v.as_vec() {
                // each item can be:
                // - a string => phrase with default return type = section_name
                // - a mapping of one entry "phrase" -> RHS
                for item in items {
                    match item {
                        Yaml::String(phrase_str) => {
                            // default return_spec = Type(section_name)
                            let (regex, params) =
                                build_regex_for_phrase(phrase_str, &param_re).map_err(|e| {
                                    SentenceParserError::RegexError(phrase_str.clone(), e.to_string())
                                })?;
                            phrases.push(PhraseConfig {
                                pattern: phrase_str.clone(),
                                regex,
                                parameters: params,
                                return_spec: ReturnSpec::Type(section_name.clone()),
                                section: section_name.clone(),
                            });
                        }
                        Yaml::Hash(map) => {
                            // expect each mapping to have a single entry phrase -> RHS
                            for (mk, mv) in map {
                                let phrase_text = match mk {
                                    Yaml::String(s) => s.clone(),
                                    _ => {
                                        return Err(SentenceParserError::InvalidPattern(
                                            "Phrase key must be a string".to_string(),
                                        ))
                                    }
                                };

                                let return_spec = parse_rhs_to_return_spec(mv, &section_name)?;
                                let (regex, params) =
                                    build_regex_for_phrase(&phrase_text, &param_re).map_err(
                                        |e| {
                                            SentenceParserError::RegexError(
                                                phrase_text.clone(),
                                                e.to_string(),
                                            )
                                        },
                                    )?;

                                phrases.push(PhraseConfig {
                                    pattern: phrase_text.clone(),
                                    regex,
                                    parameters: params,
                                    return_spec,
                                    section: section_name.clone(),
                                });
                            }
                        }
                        other => {
                            return Err(SentenceParserError::InvalidPattern(format!(
                                "Unsupported item in section {}: {:?}",
                                section_name, other
                            )));
                        }
                    }
                }
            } else {
                return Err(SentenceParserError::InvalidPattern(format!(
                    "Section {} must be a sequence",
                    section_name
                )));
            }
        }

        Ok(Self {
            phrases,
            type_patterns,
        })
    }

    // entrypoint: process a DokeNode
    fn process_with_depth(&self, node: &mut DokeNode, frontmatter: &HashMap<String, GodotValue>, depth: usize) {
        if depth > MAX_RECURSION_DEPTH {
            node.state = DokeNodeState::Error(Box::new(SentenceParserError::InvalidPattern(
                "Maximum recursion depth exceeded".to_string(),
            )));
            return;
        }

        if !matches!(node.state, DokeNodeState::Unresolved) {
            return;
        }

        let statement = node.statement.trim();

        // optional optimization: look for expected sentence type in parse_data
        let expected_type = node.parse_data.get("sentence_type").and_then(|v| {
            if let GodotValue::String(s) = v { Some(s.as_str()) } else { None }
        });

        let phrases_to_check: Vec<&PhraseConfig> = if let Some(t) = expected_type {
            // use phrases whose return_spec Type matches t, or whose section equals t
            self.phrases.iter().filter(|p| {
                match &p.return_spec {
                    ReturnSpec::Type(name) => name == t || p.section == t,
                    _ => p.section == t || match &p.return_spec { ReturnSpec::Type(n) => n == t, _=>false }
                }
            }).collect()
        } else {
            self.phrases.iter().collect()
        };

        // collect matches and parse errors
        let mut potential_matches: Vec<(&PhraseConfig, HashMap<String, String>)> = Vec::new();
        let mut parsing_errors: Vec<SentenceParseError> = Vec::new();

        for phrase in phrases_to_check {
            match match_phrase_exact(statement, phrase) {
                Ok(raw_params) => {
                    potential_matches.push((phrase, raw_params));
                }
                Err(e) => {
                    parsing_errors.push(e);
                }
            }
        }

        if potential_matches.is_empty() {
            // convert parsing_errors into Hypotheses
            if !parsing_errors.is_empty() {
                let mut hypos: Vec<Box<dyn Hypo>> = Vec::new();
                for err in parsing_errors {
                    hypos.push(Box::new(ErrorHypo { error: err, statement: node.statement.clone() }) as Box<dyn Hypo>);
                }
                node.state = DokeNodeState::Hypothesis(hypos);
                return;
            } else {
                // absolutely no matches
                node.state = DokeNodeState::Hypothesis(vec![Box::new(ErrorHypo {
                    error: SentenceParseError::NoMatch(node.statement.clone()),
                    statement: node.statement.clone(),
                })]);
                return;
            }
        }

        // choose best match by specificity
        potential_matches.sort_by_key(|(phrase, _)| phrase_specificity(phrase));
        // we want the most specific, which is last after ascending sort
        let (best_phrase, raw_params) = potential_matches.pop().unwrap();

        // parse parameter values, create constituent nodes for complex types
        let mut parsed_params: HashMap<String, GodotValue> = HashMap::new();
        let mut constituent_nodes: HashMap<String, DokeNode> = HashMap::new();

        for param_def in &best_phrase.parameters {
            if let Some(raw_val) = raw_params.get(&param_def.name) {
                if is_basic_type(&param_def.param_type) {
                    match parse_basic_parameter(raw_val, &param_def.param_type) {
                        Ok(gv) => {
                            parsed_params.insert(param_def.name.clone(), gv);
                        }
                        Err(e) => {
                            node.parse_data.insert(
                                format!("{}_error", param_def.name),
                                GodotValue::String(e.clone()),
                            );
                        }
                    }
                } else {
                    // complex type: try enum match first
                    if let Some(pats) = self.type_patterns.get(&param_def.param_type) {
                        let mut matched = false;
                        for (r, gv) in pats {
                            if r.is_match(raw_val.trim()) {
                                parsed_params.insert(param_def.name.clone(), gv.clone());
                                matched = true;
                                break;
                            }
                        }
                        if matched {
                            continue;
                        }
                    }

                    // else create constituent node for recursion
                    let mut child = create_constituent_node(raw_val, &param_def.param_type);
                    // set expected type for child
                    child.parse_data.insert("sentence_type".to_string(), GodotValue::String(param_def.param_type.clone()));
                    self.process_with_depth(&mut child, frontmatter, depth + 1);
                    constituent_nodes.insert(param_def.name.clone(), child);
                }
            }
        }

        // attach constituents
        node.constituents.extend(constituent_nodes);

        // build result according to return_spec
        let result = match &best_phrase.return_spec {
            ReturnSpec::Type(ty) => {
                SentenceResult::new_type(ty.clone(), parsed_params)
            }
            ReturnSpec::Literal(lv) => {
                // if format-like literal, but it's Literal already computed -> use as-is
                SentenceResult::new_literal(lv.clone(), parsed_params)
            }
            ReturnSpec::Format(fmt) => {
                // perform named substitution using parsed_params then frontmatter
                let final_str = perform_format_string(fmt, &parsed_params, frontmatter);
                SentenceResult::new_literal(GodotValue::String(final_str), parsed_params)
            }
        };

        node.state = DokeNodeState::Resolved(Box::new(result));
    }
}

// DokeParser trait impl
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
            r.parse::<f64>().map(GodotValue::Float).unwrap_or(GodotValue::Float(0.0))
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
    matches!(param_type.to_lowercase().as_str(), "int" | "float" | "bool" | "string")
}

fn parse_basic_parameter(value: &str, param_type: &str) -> Result<GodotValue, String> {
    match param_type.to_lowercase().as_str() {
        "int" => {
            // support hex/octal/binary prefixes
            if value.starts_with("0b") || value.starts_with("0B") {
                i64::from_str_radix(&value[2..], 2).map(GodotValue::Int).map_err(|e| e.to_string())
            } else if value.starts_with("0o") || value.starts_with("0O") {
                i64::from_str_radix(&value[2..], 8).map(GodotValue::Int).map_err(|e| e.to_string())
            } else if value.starts_with("0x") || value.starts_with("0X") {
                i64::from_str_radix(&value[2..], 16).map(GodotValue::Int).map_err(|e| e.to_string())
            } else {
                value.parse::<i64>().map(GodotValue::Int).map_err(|e| e.to_string())
            }
        }
        "float" => value.parse::<f64>().map(GodotValue::Float).map_err(|e| e.to_string()),
        "bool" => match value.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(GodotValue::Bool(true)),
            "false" | "no" | "0" => Ok(GodotValue::Bool(false)),
            _ => Err(format!("Invalid boolean value: {}", value)),
        },
        "string" => Ok(GodotValue::String(value.to_string())),
        _ => Err(format!("Unknown basic type: {}", param_type)),
    }
}

fn create_constituent_node(value: &str, param_type: &str) -> DokeNode {
    DokeNode {
        statement: value.to_string(),
        state: DokeNodeState::Unresolved,
        children: Vec::new(),
        parse_data: HashMap::new(),
        constituents: HashMap::new(),
    }
}

fn perform_format_string(fmt: &str, params: &HashMap<String, GodotValue>, front: &HashMap<String, GodotValue>) -> String {
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
            let parts: Vec<String> = m.iter().map(|(k, gv)| format!("{}:{}", k, godot_value_to_string(gv))).collect();
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
fn build_regex_for_phrase(phrase: &str, param_re: &Regex) -> Result<(Regex, Vec<ParameterDefinition>), Box<dyn Error>> {
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

        let name = cap.get(1).unwrap().as_str().trim().to_string();
        let param_type = cap.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_else(|| "string".to_string());

        // add capture group by type
        let capture_group = match param_type.to_lowercase().as_str() {
            "int" => r"([-+]?(?:0[bB][01]+|0[oO][0-7]+|0[xX][0-9a-fA-F]+|\d+))".to_string(),
            "float" => r"([-+]?(?:\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?)".to_string(),
            "bool" => r"(true|false|yes|no|1|0)".to_string(),
            _ => r"(.+?)".to_string(), // non-greedy default
        };

        regex_pattern.push_str(&capture_group);
        parameters.push(ParameterDefinition { name, param_type });

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
fn match_phrase_exact(statement: &str, phrase: &PhraseConfig) -> Result<HashMap<String, String>, SentenceParseError> {
    let caps = phrase.regex.captures(statement).ok_or(SentenceParseError::NoMatch(phrase.pattern.clone()))?;
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
fn parse_rhs_to_return_spec(node: &Yaml, section_default: &str) -> Result<ReturnSpec, SentenceParserError> {
    match node {
        Yaml::Null => Ok(ReturnSpec::Type(section_default.to_string())),
        Yaml::String(s) => {
            let s_trim = s.trim();
            // l"..." literal string
            if let Some(inner) = s_trim.strip_prefix("l\"").and_then(|r| r.strip_suffix('\"')) {
                return Ok(ReturnSpec::Literal(GodotValue::String(inner.to_string())));
            }
            // f"..." format string
            if let Some(inner) = s_trim.strip_prefix("f\"").and_then(|r| r.strip_suffix('\"')) {
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
            if matches!(s_trim.to_lowercase().as_str(), "true" | "false" | "yes" | "no" | "1" | "0") {
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
        other => Err(SentenceParserError::InvalidPattern(format!("Unsupported RHS: {:?}", other))),
    }
}

// ----------------- SentenceResult + DokeOut implementation -----------------

#[derive(Debug)]
struct SentenceResult {
    output_type: String, // type name when resource
    parameters: HashMap<String, GodotValue>,
    literal_value: Option<GodotValue>, // when a phrase returns a literal value instead of a Resource
}

impl SentenceResult {
    fn new_type(t: String, params: HashMap<String, GodotValue>) -> Self {
        Self { output_type: t, parameters: params, literal_value: None }
    }
    fn new_literal(val: GodotValue, mut params: HashMap<String, GodotValue>) -> Self {
        // store literal value in a special key maybe as literal_value
        Self { output_type: "".to_string(), parameters: params, literal_value: Some(val) }
    }
}

impl DokeOut for SentenceResult {
    fn kind(&self) -> &'static str { "SentenceResult" }

    fn to_godot(&self) -> GodotValue {
        if let Some(lit) = &self.literal_value {
            lit.clone()
        } else {
            GodotValue::Resource {
                type_name: self.output_type.clone(),
                fields: self.parameters.clone(),
            }
        }
    }

    fn use_child(&mut self, child: GodotValue) -> Result<(), Box<dyn Error>> {
        match self.parameters.entry("children".into()) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                if let GodotValue::Array(a) = e.get_mut() {
                    a.push(child);
                    Ok(())
                } else {
                    Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "children field is not an array")))
                }
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(GodotValue::Array(vec![child]));
                Ok(())
            }
        }
    }

    fn use_constituent(&mut self, name: &str, value: GodotValue) -> Result<(), Box<dyn Error>> {
        self.parameters.insert(name.to_string(), value);
        Ok(())
    }
}

// ----------------- Parsing error types & error hypo -----------------

#[derive(Debug)]
pub enum SentenceParseError {
    NoMatch(String),
    TypeParseError(String, String, String), // type, value, err
    UnknownEnumValue(String, String), // type, attempted enum key
    MaxRecursionDepthExceeded,
}

impl std::fmt::Display for SentenceParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SentenceParseError::NoMatch(p) => write!(f, "Pattern '{}' did not match", p),
            SentenceParseError::TypeParseError(t, v, e) => write!(f, "Type parse error for {} value '{}' : {}", t, v, e),
            SentenceParseError::UnknownEnumValue(t, v) => write!(f, "Unknown enum value '{}' for type '{}'", v, t),
            SentenceParseError::MaxRecursionDepthExceeded => write!(f, "Maximum recursion depth exceeded"),
        }
    }
}

impl Error for SentenceParseError {}

#[derive(Debug)]
struct ErrorHypo {
    error: SentenceParseError,
    statement: String,
}

impl Hypo for ErrorHypo {
    fn kind(&self) -> &'static str { "SentenceParseError" }
    fn confidence(&self) -> f32 { -1.0 }
    fn promote(self: Box<Self>) -> Result<Box<dyn DokeOut>, Box<dyn Error>> {
        Err(Box::new(self.error))
    }
}

// ----------------- Utility: parse RHS and substitution helpers -----------------

// (already defined above) perform_format_string & godot_value_to_string

// ----------------- End of file -----------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crate::parsers::sentence::SentenceParser;
    use crate::semantic::{DokeNode, DokeNodeState, GodotValue};
    use crate::DokeParser;

    // Helper: process a statement and return the resolved GodotValue directly
    fn resolve_node(parser: &SentenceParser, statement: &str) -> GodotValue {
        let mut node = DokeNode {
            statement: statement.to_string(),
            state: DokeNodeState::Unresolved,
            children: Vec::new(),
            parse_data: HashMap::new(),
            constituents: HashMap::new(),
        };
        parser.process(&mut node, &HashMap::new());

        if let DokeNodeState::Resolved(result) = &node.state {
            result.to_godot()
        } else {
            panic!("Node was not resolved: {:?}", node.state);
        }
    }

    #[test]
    fn test_basic_damage_effect_default_type() {
        let yaml = r#"
DamageEffect:
  - "Deals {damage: int}"
"#;
        let parser = SentenceParser::from_yaml(yaml).unwrap();
        let val = resolve_node(&parser, "Deals 42");

        if let GodotValue::Resource { type_name, fields } = val {
            assert_eq!(type_name, "DamageEffect");
            assert_eq!(fields.get("damage"), Some(&GodotValue::Int(42)));
        } else {
            panic!("Expected Resource, got {:?}", val);
        }
    }

    #[test]
    fn test_damage_effect_custom_type() {
        let yaml = r#"
DamageEffect:
  - "Deals {damage: int} {target: Target}" : TargetedDamageEffect

enum Target:
  - "to Enemies": 1
  - "to Allies": 2
  - "": 3
"#;
        let parser = SentenceParser::from_yaml(yaml).unwrap();
        let val = resolve_node(&parser, "Deals 99 to Enemies");

        if let GodotValue::Resource { type_name, fields } = val {
            assert_eq!(type_name, "TargetedDamageEffect");
            assert_eq!(fields.get("damage"), Some(&GodotValue::Int(99)));
            assert_eq!(fields.get("target"), Some(&GodotValue::Int(1)));
        } else {
            panic!("Expected Resource, got {:?}", val);
        }
    }

    #[test]
    fn test_literal_return() {
        let yaml = r#"
MessageEffect:
  - "Print Hello" : l"Message"
"#;
        let parser = SentenceParser::from_yaml(yaml).unwrap();
        let val = resolve_node(&parser, "Print Hello");

        assert_eq!(val, GodotValue::String("Message".to_string()));
    }

    #[test]
    fn test_formatted_return() {
        let yaml = r#"
MessageEffect:
  - "Print {message: String}" : f"Message {message}"
"#;
        let parser = SentenceParser::from_yaml(yaml).unwrap();
        let val = resolve_node(&parser, "Print HelloWorld");

        assert_eq!(val, GodotValue::String("Message HelloWorld".to_string()));
    }
}
