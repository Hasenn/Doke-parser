use crate::{DokeNode, DokeParser, GodotValue, semantic::DokeNodeState};
use std::collections::HashMap;

/// A parser that prints the node tree for debugging purposes.
/// Can be added anywhere in a pipeline with `.add(DebugPrinter)`.
pub struct DebugPrinter;

impl DebugPrinter {
    fn state_emoji(state: &DokeNodeState) -> &'static str {
        match state {
            DokeNodeState::Unresolved => "❓",
            DokeNodeState::Hypothesis(_) => "💡",
            DokeNodeState::Resolved(_) => "✅",
            DokeNodeState::Error(_) => "❌",
        }
    }

    fn print_node(node: &DokeNode, indent: usize) {
        let padding = "  ".repeat(indent);
        println!(
            "{}{} {}",
            padding,
            Self::state_emoji(&node.state),
            node.statement
        );

        for child in &node.children {
            Self::print_node(child, indent + 1);
        }
    }
}

impl DokeParser for DebugPrinter {
    fn process(&self, node: &mut DokeNode, _frontmatter: &HashMap<String, GodotValue>) {
        // Recursively print the node starting from here
        Self::print_node(node, 0);
    }
}
