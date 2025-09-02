# DokePipe

A powerful and extensible semantic parsing pipeline for Markdown documents. DokePipe transforms standard Markdown text into a structured, semantical document (`DokeDocument`) that can be validated and converted into Godot-friendly data structures (`GodotValue`).

## Features

- **Frontmatter Extraction**: Automatically parses YAML frontmatter from Markdown documents.
- **Semantic Parsing**: Converts Markdown AST into a customizable node tree (`DokeNode`).
- **Extensible Pipeline**: Add custom parsers to interpret and transform document nodes.
- **Hypothesis System**: Supports multiple competing interpretations of content with confidence scoring.
- **Godot Integration**: Outputs data in `GodotValue` enum format, ready for use with Godot's GDNative or GDExtension interfaces.
- **Validation**: Built-in validation system to ensure document structure and content integrity.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
doke = "0.1.0"
```

## Usage

### Making and running a DokePipe

```rs
use std::io::{self, Read};
use doke::{parsers, DokePipe, GodotValue};

fn main() -> Result<(), std::io::Error> {
    // Read entire stdin into a string
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Build the pipeline with DebugPrinter included
    let pipe = DokePipe::new()
        .add(parsers::FrontmatterTemplateParser)
        .add(parsers::DebugPrinter); // prints nodes with emojis for debugging

    // Run the pipeline on the input Markdown (Mostly for debugging, use validate to get the data !)
    let _doc = pipe.run_markdown(&input);
    // Get the godot values from the document
    let _val : Vec<GodotValue> = pipe.validate(&input).unwrap();
    dbg!(_doc);
    Ok(())
}
```

### Making your own semantic parsers


Example from the Frontmatter templating parser provided in parsers::FrontmatterTemplateParser
this parser changes the statement inside a node for later parsers in the pipe

```rs
use std::collections::HashMap;
use regex::Regex;


use crate::{semantic::{DokeNode, DokeParser}, GodotValue};

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

```



For parsers that create GodotValues, they can either set the DokeNode's state to `DokeNodeState::Hypothesis(Box::new(my_hypothesis))` with an Hypothesis that has the `Hypo` trait and can be promoted to a `dyn DokeOut` boxed object. That would put an hypothesis on the node.

If the parser is sure about the node, it can directly set its state to `DokeNodeState::Resolved(dyn DokeOut)` and pass it a boxed object of a type that implements `DokeOut` and therefore defines a `to_godot` method

```rs
// Add to semantic.rs or your main file

// ----------------- Hello World Parser -----------------

#[derive(Debug)]
pub struct GreeterResource {
    message: String,
}

impl GreeterResource {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

impl DokeOut for GreeterResource {
    fn kind(&self) -> &'static str {
        "Greeter"
    }

    fn to_godot(&self) -> GodotValue {
        GodotValue::Resource {
            type_name: "Greeter".to_string(),
            fields: {
                let mut map = HashMap::new();
                map.insert("message".to_string(), GodotValue::String(self.message.clone()));
                map
            },
        }
    }
}

#[derive(Debug)]
pub struct HelloWorldHypothesis {
    confidence: f32,
    message: String,
}

impl HelloWorldHypothesis {
    pub fn new(message: String, confidence: f32) -> Self {
        Self { confidence, message }
    }
}

impl Hypo for HelloWorldHypothesis {
    fn kind(&self) -> &'static str {
        "HelloWorldHypothesis"
    }

    fn confidence(&self) -> f32 {
        self.confidence
    }

    fn promote(self: Box<Self>) -> Result<Box<dyn DokeOut>, Box<dyn Error>> {
        Ok(Box::new(GreeterResource::new(self.message)))
    }
}

#[derive(Debug)]
pub struct HelloWorldParser;

impl DokeParser for HelloWorldParser {
    fn process(&self, node: &mut DokeNode, _frontmatter: &HashMap<String, GodotValue>) {
        // Skip if already resolved or in error state
        if !matches!(node.state, DokeNodeState::Unresolved) {
            return;
        }

        // Check if this node contains "Hello World"
        if node.statement.contains("Hello World") || node.statement.contains("hello world") {
            let confidence = if node.statement.contains("Hello World") {
                1.0 // High confidence for exact match
            } else {
                0.8 // Slightly lower confidence for case-insensitive match
            };

            let hypothesis = HelloWorldHypothesis::new(node.statement.clone(), confidence);
            node.state = DokeNodeState::Hypothesis(vec![Box::new(hypothesis)]);
        }
        
        // Process children recursively
        for child in &mut node.children {
            self.process(child, _frontmatter);
        }
    }
}
```