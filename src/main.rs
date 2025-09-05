use doke::{DokePipe, GodotValue, parsers};
use core::error;
use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    
    if args.len() != 3 || args[1] != "--sentence" {
        eprintln!("Usage: {} --sentence <sentence_file_path>", args[0]);
        std::process::exit(1);
    }
    
    let sentence_path = &args[2];
    
    // Read sentence configuration from file
    let mut sentence_file = File::open(sentence_path)
        .map_err(|e| format!("Failed to open sentence file '{}': {}", sentence_path, e))?;
    
    let mut sentence_config = String::new();
    sentence_file.read_to_string(&mut sentence_config)
        .map_err(|e| format!("Failed to read sentence file '{}': {}", sentence_path, e))?;
    
    // Check if sentence config is empty
    if sentence_config.trim().is_empty() {
        return Err("Sentence configuration file is empty".into());
    }
    
    // Read entire stdin into a string
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    
    let sentence_parser = parsers::SentenceParser::from_yaml(&sentence_config)?;

    // Build the pipeline with DebugPrinter included
    let pipe = DokePipe::new()
        .add(parsers::FrontmatterTemplateParser)
        .filter_map(sentence_parser, |_,_,_| true)
        .add(parsers::DebugPrinter) // prints nodes with emojis for debugging
    ;
    
    // Get the godot values from the document
    let _res = pipe.validate(&input);
    match _res {
        Err(e) => {eprint!("{}", e);},
        Ok(val) => {dbg!(val);}
    }
    Ok(())
}