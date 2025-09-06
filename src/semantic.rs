use markdown::mdast::Node;
use std::any::Any;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Debug};
use thiserror::Error;

use crate::base_parser::Position;

// ----------------- GodotValue -----------------

#[derive(Debug, Clone, PartialEq)]
pub enum GodotValue {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<GodotValue>),
    Dict(HashMap<String, GodotValue>),
    Resource {
        type_name: String,
        fields: HashMap<String, GodotValue>,
    },
}

// ----------------- Traits -----------------

pub trait Hypo: std::fmt::Debug {
    fn kind(&self) -> &'static str;
    fn confidence(&self) -> f32 {
        1.0
    }
    fn promote(self: Box<Self>) -> Result<Box<dyn DokeOut>, Box<dyn Error>>;
}

/// Trait for things that can convert to_godot and potentially use_child
pub trait DokeOut: std::fmt::Debug {
    fn kind(&self) -> &'static str;
    fn to_godot(&self) -> GodotValue;
    fn get_asbtract_type(&self) -> Option<String> {
        None
    }
    fn use_child(&mut self, _child: GodotValue) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
    fn use_constituent(&mut self, _name: &str, _value: GodotValue) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

// ----------------- DokeNode -----------------

/// The semantic tree that parsers operate on.
/// It is made from statements parsed by the Doke base parser
///
/// They are copied into owned versions
/// so that our chain of parsers can modify a node's content
/// Or even create children nodes if they need to.
#[derive(Debug)]
pub struct DokeNode {
    /// The Statement as a string.
    ///
    /// Parsers can edit this to move some data to parse_data
    /// while preventing other parsers to have to deal with the syntax.
    pub statement: String,
    /// The state of the node : UnResolved, Resolved(result),
    /// Hypothesis(potential results), or Error(err)
    pub state: DokeNodeState,
    /// The children statements.
    pub children: Vec<DokeNode>,
    /// A bucket of Godot-Compatible data that parsers can populate and read from.
    pub parse_data: HashMap<String, GodotValue>,
    /// The constituent parts of the statement, if it takes some and a parser broke it like that.
    pub constituents: HashMap<String, DokeNode>,
    /// The position of the original statement in the source string
    /// For constituents as of now, it is the position of the whole statement.
    /// Only used for error reporting
    pub span: Position,
}

/// The state of an unparsed, parsed, maybe parsed, or definitely wrong statement.
#[derive(Debug)]
pub enum DokeNodeState {
    /// Nodes start Unresolved
    Unresolved,
    /// Parsers that are not sure about their guess
    /// can push an Hypothesis here
    ///
    /// If a later parser is more confident, or marks the node as Resolved
    /// the hypothesis will be forgotten
    ///
    /// If not, the Validation pass at the end of the pipe will try to promote() the hypothesis
    /// into a `DokeOut` and build its godot value.
    Hypothesis(Vec<Box<dyn Hypo>>),
    /// A resolved node has been fully recognized as something by a parser.
    Resolved(Box<dyn DokeOut>),
    /// A parser that knows for sure that the statement is an invalid construct, can
    /// set this state to an Error.
    /// Further parsers should ignore the node and keep going.
    /// A parser erroring on a node because it is not formed like what he parses
    /// Can choose to push a negative confidence Hypothesis that resolves to
    /// an Error.
    Error(Box<dyn Error>),
}

// ----------------- Parsers -----------------

/// Updated trait: parsers now get a reference to frontmatter
pub trait DokeParser: Debug + Send + Sync {
    fn process(&self, node: &mut DokeNode, frontmatter: &HashMap<String, GodotValue>);
}
// ----------------- Error Types -----------------

#[derive(Debug, Error)]
pub enum DokeValidationError {
    #[error("Validation error at node: {0} : {1}")]
    NodeError(String, String),
    #[error("Missing required field '{0}' in resource '{1}'")]
    MissingField(String, String),
    #[error("Invalid field type for '{0}' in resource '{1}': expected {2}, got {3}")]
    InvalidFieldType(String, String, String, String),
    #[error("(Promoted Err) {0} - position {1}")]
    HypothesisPromotionFailed(#[source] Box<dyn Error>, Position),
    #[error("Unresolved node: {0}")]
    UnresolvedNode(String),
    #[error("Multiple errors occurred during validation: {0}")]
    MultipleErrors(#[from] DokeErrors),
    #[error("Failed to use child: {0}")]
    ChildUsageFailed(#[source] Box<dyn Error>),
    #[error("Dynamic Error")]
    DynamicError(#[from] Box<dyn std::error::Error>),
}

// Wrapper struct for multiple errors
#[derive(Debug, Error)]
pub struct DokeErrors(Vec<DokeValidationError>);

impl From<Vec<DokeValidationError>> for DokeErrors {
    fn from(errors: Vec<DokeValidationError>) -> Self {
        DokeErrors(errors)
    }
}

impl fmt::Display for DokeErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "")?;
        for (i, error) in self.0.iter().enumerate() {
            writeln!(f, "  {}. {}", i + 1, error)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ErrorHypo<Er: Error> {
    error: Er,
}

impl<Er: Error + 'static> Hypo for ErrorHypo<Er> {
    fn kind(&self) -> &'static str {
        "Error"
    }
    fn confidence(&self) -> f32 {
        -1.
    }
    fn promote(self: Box<Self>) -> Result<Box<dyn DokeOut>, Box<dyn Error>> {
        Err(Box::new(self.error))
    }
}

#[derive(Debug, Error)]
pub enum GodotValueError {
    #[error("Tried to add a child to a {0}")]
    InvalidChild(String),
}

impl DokeOut for GodotValue {
    fn kind(&self) -> &'static str {
        match &self {
            GodotValue::Nil => "Null",
            GodotValue::Bool(_) => "Bool",
            GodotValue::Int(_) => "Int",
            GodotValue::Float(_) => "Float",
            GodotValue::String(_) => "String",
            GodotValue::Array(_) => "Array",
            GodotValue::Dict(_) => "Dict",
            GodotValue::Resource {
                type_name: _,
                fields: _,
            } => "Resource",
        }
    }
    fn to_godot(&self) -> GodotValue {
        self.clone()
    }
    fn use_child(&mut self, _child: GodotValue) -> Result<(), Box<dyn Error>> {
        match self {
            GodotValue::Nil
            | GodotValue::Bool(_)
            | GodotValue::Int(_)
            | GodotValue::Float(_)
            | GodotValue::String(_) => Err(Box::new(GodotValueError::InvalidChild(
                self.kind().to_owned(),
            ))),
            GodotValue::Array(v) => {
                v.push(_child);
                Ok(())
            }
            GodotValue::Dict(h) => {
                h.insert("children".into(), _child);
                Ok(())
            }
            GodotValue::Resource {
                type_name: _,
                fields,
            } => {
                match &mut fields
                    .entry("children".into())
                    .or_insert(GodotValue::Array(vec![]))
                {
                    GodotValue::Array(godot_values) => {
                        godot_values.push(_child);
                        Ok(())
                    }
                    _ => Err(Box::new(GodotValueError::InvalidChild(
                        "Can't add child to resource : children field is not empty or an array"
                            .into(),
                    ))),
                }
            }
        }
    }
}

// ----------------- DokeValidate Parser -----------------

pub struct DokeValidate {
    errors: Vec<DokeValidationError>,
}

impl DokeValidate {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn validate_tree(
        root_nodes: &mut [DokeNode],
        frontmatter: &HashMap<String, GodotValue>,
    ) -> Result<Vec<GodotValue>, DokeValidationError> {
        let mut validator = Self::new();
        let results: Vec<Result<GodotValue, DokeValidationError>> = root_nodes
            .iter_mut()
            .map(|n| validator.process_node(n, frontmatter))
            .collect();

        // Flatten results
        let mut ok_values = Vec::new();
        for r in results {
            match r {
                Ok(v) => ok_values.push(v),
                Err(e) => validator.errors.push(e),
            }
        }

        if validator.errors.is_empty() {
            Ok(ok_values)
        } else if validator.errors.len() == 1 {
            Err(validator.errors.remove(0))
        } else {
            Err(DokeValidationError::MultipleErrors(DokeErrors(
                validator.errors,
            )))
        }
    }

    fn process_node(
        &mut self,
        node: &mut DokeNode,
        frontmatter: &HashMap<String, GodotValue>,
    ) -> Result<GodotValue, DokeValidationError> {
        let mut child_values = Vec::new();
        let mut constituent_values: HashMap<String, GodotValue> = HashMap::new();
        for child in &mut node.children {
            match self.process_node(child, frontmatter) {
                Ok(v) => child_values.push(v),
                Err(e) => return Err(e),
            };
        }
        for (name, constituent) in &mut node.constituents {
            match self.process_node(constituent, frontmatter) {
                Ok(v) => constituent_values.insert(name.into(), v),
                Err(e) => return Err(e),
            };
        }

        match &mut node.state {
            DokeNodeState::Unresolved => {
                Err(DokeValidationError::UnresolvedNode(node.statement.clone()))
            }
            DokeNodeState::Hypothesis(hypotheses) => {
                let best_index = hypotheses
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| {
                        a.confidence()
                            .partial_cmp(&b.confidence())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(i, _)| i);

                if let Some(best_index) = best_index {
                    let hypo = hypotheses.remove(best_index);
                    let mut resolved = hypo.promote().map_err(|e| {
                        DokeValidationError::HypothesisPromotionFailed(e, node.span.clone())
                    })?;

                    for child in &child_values {
                        resolved
                            .use_child(child.clone())
                            .map_err(DokeValidationError::ChildUsageFailed)?;
                    }
                    for (name, value) in &constituent_values {
                        resolved.use_constituent(name, value.clone())?;
                    }

                    node.state = DokeNodeState::Resolved(resolved);
                    if let DokeNodeState::Resolved(resolved) = &node.state {
                        Ok(resolved.to_godot())
                    } else {
                        unreachable!()
                    }
                } else {
                    Err(DokeValidationError::UnresolvedNode(node.statement.clone()))
                }
            }
            DokeNodeState::Resolved(resolved) => {
                for child in &child_values {
                    resolved
                        .use_child(child.clone())
                        .map_err(DokeValidationError::ChildUsageFailed)?;
                }
                for (name, value) in &constituent_values {
                    resolved.use_constituent(name, value.clone())?;
                }
                Ok(resolved.to_godot())
            }
            DokeNodeState::Error(e) => Err(DokeValidationError::NodeError(
                node.statement.clone(),
                format!("{}", e),
            )),
        }
    }
}
