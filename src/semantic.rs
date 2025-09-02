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

/// Recursive pipeline now passes frontmatter to every parser call
pub fn run_pipeline(
    root_nodes: &mut [DokeNode],
    parsers: &[Box<dyn DokeParser>],
    frontmatter: &HashMap<String, GodotValue>,
) {
    for parser in parsers {
        for node in root_nodes.iter_mut() {
            process_node_recursively(node, parser.as_ref(), frontmatter);
        }
    }
}

fn process_node_recursively(
    node: &mut DokeNode,
    parser: &dyn DokeParser,
    frontmatter: &HashMap<String, GodotValue>,
) {
    parser.process(node, frontmatter);
    for child in &mut node.children {
        process_node_recursively(child, parser, frontmatter);
    }
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
        let results = validator.process_nodes(root_nodes, frontmatter);

        if validator.errors.is_empty() {
            Ok(results)
        } else if validator.errors.len() == 1 {
            Err(validator.errors.remove(0))
        } else {
            Err(DokeValidationError::MultipleErrors(validator.errors))
        }
    }

    fn process_nodes(
        &mut self,
        nodes: &mut [DokeNode],
        frontmatter: &HashMap<String, GodotValue>,
    ) -> Vec<GodotValue> {
        nodes.iter_mut().map(|node| self.process_node(node, frontmatter)).collect()
    }

    fn process_node(&mut self, node: &mut DokeNode, frontmatter: &HashMap<String, GodotValue>) -> GodotValue {
        let child_values = self.process_nodes(&mut node.children, frontmatter);

        match &mut node.state {
            DokeNodeState::Unresolved => {
                self.errors.push(DokeValidationError::UnresolvedNode(node.statement.clone()));
                GodotValue::Nil
            }
            DokeNodeState::Hypothesis(hypotheses) => {
                let best_index = hypotheses.iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.confidence().partial_cmp(&b.confidence()).unwrap())
                    .map(|(i, _)| i);

                if let Some(best_index) = best_index {
                    let hypo = hypotheses.remove(best_index);
                    match hypo.promote() {
                        Ok(mut resolved) => {
                            for child in &child_values {
                                if let Err(e) = resolved.use_child(child.clone()) {
                                    self.errors.push(DokeValidationError::ChildUsageFailed(e));
                                }
                            }
                            node.state = DokeNodeState::Resolved(resolved);
                            if let DokeNodeState::Resolved(resolved) = &node.state {
                                resolved.to_godot()
                            } else { GodotValue::Nil }
                        }
                        Err(e) => {
                            self.errors.push(DokeValidationError::HypothesisPromotionFailed(e));
                            GodotValue::Nil
                        }
                    }
                } else {
                    self.errors.push(DokeValidationError::UnresolvedNode(node.statement.clone()));
                    GodotValue::Nil
                }
            }
            DokeNodeState::Resolved(resolved) => {
                for child in &child_values {
                    if let Err(e) = resolved.use_child(child.clone()) {
                        self.errors.push(DokeValidationError::ChildUsageFailed(e));
                    }
                }
                resolved.to_godot()
            }
            DokeNodeState::Error(e) => {
                self.errors.push(DokeValidationError::NodeError(node.statement.clone(), format!("{}", e)));
                GodotValue::Nil
            }
        }
    }
}