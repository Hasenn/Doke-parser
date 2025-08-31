use doke_parser::{DokeDocument, DokeParser, DokeStatement};
use std::io::{self, Read};
use std::process;

fn main() {
    let mut input = String::new();
    
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("Error reading from stdin: {}", e);
        process::exit(1);
    }
    
    if input.trim().is_empty() {
        eprintln!("Error: No input provided");
        process::exit(1);
    }
    
    let parser = DokeParser::new();
    
    match parser.parse(&input) {
        Ok(document) => {
            println!("✅ Successfully parsed Doke document");
            println!("📄 Found {} top-level statements:", document.statements.len());
            println!("{}", "─".repeat(80));
            
            for (i, statement) in document.statements.iter().enumerate() {
                print_statement(statement, i, 0, &input);
            }
            
            let total_statements = count_total_statements(&document);
            println!("{}", "─".repeat(80));
            println!("📊 Total statements (including children): {}", total_statements);
        }
        Err(e) => {
            eprintln!("❌ Parse error: {}", e);
            process::exit(1);
        }
    }
}

fn print_statement(statement: &DokeStatement, index: usize, depth: usize, source: &str) {
    let indent = "  ".repeat(depth);
    let number = if depth == 0 { 
        format!("{}.", index + 1) 
    } else { 
        "•".to_string() 
    };
    
    println!("{}{} {}", indent, number, statement.content);
    
    // Print content position
    if let Some((start, end)) = statement.content_position {
        if end <= source.len() {
            let text_slice = &source[start..end];
            let cleaned_slice = text_slice.replace('\n', "\\n").replace('\r', "\\r");
            println!("{}   📍 Content: {}..{} -> \"{}\"", indent, start, end, cleaned_slice);
        }
    }
    
    // Print children position if available
    if let Some((start, end)) = statement.children_position {
        if end <= source.len() {
            let text_slice = &source[start..end];
            let cleaned_slice = text_slice.replace('\n', "\\n").replace('\r', "\\r");
            println!("{}   🧒 Children area: {}..{} -> \"{}\"", indent, start, end, cleaned_slice);
        }
    }
    
    // Print all children recursively
    if !statement.children.is_empty() {
        println!("{}   👥 Children ({}):", indent, statement.children.len());
        for (i, child) in statement.children.iter().enumerate() {
            print_statement(child, i, depth + 2, source);
        }
    }
    
    if depth == 0 {
        println!();
    }
}

fn count_total_statements(document: &DokeDocument) -> usize {
    let mut count = document.statements.len();
    
    for statement in &document.statements {
        count += count_children(&statement.children);
    }
    
    count
}

fn count_children(children: &[DokeStatement]) -> usize {
    let mut count = children.len();
    
    for child in children {
        count += count_children(&child.children);
    }
    
    count
}