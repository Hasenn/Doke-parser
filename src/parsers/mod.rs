mod debug;
mod sentence;
pub use sentence::SentenceParser;
pub use debug::DebugPrinter;
use regex::Regex;
use std::collections::HashMap;

use crate::{
    GodotValue,
    semantic::{DokeNode, DokeParser},
};

pub struct FrontmatterTemplateParser;

impl DokeParser for FrontmatterTemplateParser {
    fn process(&self, node: &mut DokeNode, frontmatter: &HashMap<String, GodotValue>) {
        let re = Regex::new(r"\{([a-zA-Z0-9_ ]+)\}").unwrap();

        // Normalize frontmatter keys: lowercase + replace spaces with '_'
        let normalized_map: HashMap<String, &GodotValue> = frontmatter
            .iter()
            .map(|(k, v)| (k.to_lowercase().replace(' ', "_"), v))
            .collect();

        // Replace placeholders
        let new_statement = re.replace_all(&node.statement, |caps: &regex::Captures| {
            let key_raw = &caps[1];
            let key = key_raw.to_lowercase().replace(' ', "_"); // normalize placeholder

            if let Some(value) = normalized_map.get(&key) {
                match value {
                    GodotValue::Int(i) => i.to_string(),
                    GodotValue::Float(f) => f.to_string(),
                    GodotValue::String(s) => s.clone(),
                    GodotValue::Bool(b) => b.to_string(),
                    _ => format!("{{{}}}", key_raw), // fallback
                }
            } else {
                format!("{{{}}}", key_raw) // keep placeholder if not found
            }
        });

        node.statement = new_statement.to_string();

        // Recursively process children
        for child in &mut node.children {
            self.process(child, frontmatter);
        }
    }
}
