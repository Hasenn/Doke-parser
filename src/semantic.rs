use std::collections::HashMap;
use std::error::Error;
use thiserror::Error;

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
    fn confidence(&self) -> f32 { 1.0 }
    fn promote(self: Box<Self>) -> Result<Box<dyn DokeOut>, Box<dyn Error>>;
}

/// Updated DokeOut trait remains unchanged
pub trait DokeOut: std::fmt::Debug {
    fn kind(&self) -> &'static str;
    fn to_godot(&self) -> GodotValue;
    fn use_child(&mut self, _child: GodotValue) -> Result<(), Box<dyn Error>> { Ok(()) }
}

// ----------------- DokeNode -----------------

#[derive(Debug)]
pub struct DokeNode {
    pub statement: String,
    pub state: DokeNodeState,
    pub children: Vec<DokeNode>,
}

#[derive(Debug)]
pub enum DokeNodeState {
    Unresolved,
    Hypothesis(Vec<Box<dyn Hypo>>),
    Resolved(Box<dyn DokeOut>),
    Error(Box<dyn Error>),
}

// ----------------- Parsers -----------------

/// Updated trait: parsers now get a reference to frontmatter
pub trait DokeParser {
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
    #[error("Failed to promote hypothesis: {0}")]
    HypothesisPromotionFailed(#[source] Box<dyn Error>),
    #[error("Unresolved node: {0}")]
    UnresolvedNode(String),
    #[error("Multiple errors occurred during validation")]
    MultipleErrors(Vec<DokeValidationError>),
    #[error("Failed to use child: {0}")]
    ChildUsageFailed(#[source] Box<dyn Error>),
}

// ----------------- DokeValidate Parser -----------------

pub struct DokeValidate {
    errors: Vec<DokeValidationError>,
}

impl DokeValidate {
    pub fn new() -> Self { Self { errors: Vec::new() } }

    pub fn validate_tree(
        root_nodes: &mut [DokeNode],
        frontmatter: &HashMap<String, GodotValue>,
    ) -> Result<Vec<GodotValue>, DokeValidationError> {
        let mut validator = Self::new();
        let results: Vec<Result<GodotValue, DokeValidationError>> =
            root_nodes.iter_mut().map(|n| validator.process_node(n, frontmatter)).collect();

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
            Err(DokeValidationError::MultipleErrors(validator.errors))
        }
    }

    fn process_node(
        &mut self,
        node: &mut DokeNode,
        frontmatter: &HashMap<String, GodotValue>,
    ) -> Result<GodotValue, DokeValidationError> {
        let mut child_values = Vec::new();
        for child in &mut node.children {
            match self.process_node(child, frontmatter) {
                Ok(v) => child_values.push(v),
                Err(e) => return Err(e),
            }
        }

        match &mut node.state {
            DokeNodeState::Unresolved => {
                Err(DokeValidationError::UnresolvedNode(node.statement.clone()))
            }
            DokeNodeState::Hypothesis(hypotheses) => {
                let best_index = hypotheses
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.confidence().partial_cmp(&b.confidence()).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(i, _)| i);

                if let Some(best_index) = best_index {
                    let hypo = hypotheses.remove(best_index);
                    let mut resolved = hypo.promote()
                        .map_err(DokeValidationError::HypothesisPromotionFailed)?;

                    for child in &child_values {
                        resolved.use_child(child.clone())
                            .map_err(DokeValidationError::ChildUsageFailed)?;
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
                    resolved.use_child(child.clone())
                        .map_err(DokeValidationError::ChildUsageFailed)?;
                }
                Ok(resolved.to_godot())
            }
            DokeNodeState::Error(e) => {
                Err(DokeValidationError::NodeError(
                    node.statement.clone(),
                    format!("{}", e),
                ))
            }
        }
    }
}
