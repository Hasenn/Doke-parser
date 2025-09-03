use doke::{DokePipe, GodotValue, parsers};
use std::io::{self, Read};

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
    let _val: Vec<GodotValue> = pipe.validate(&input).unwrap();
    dbg!(_doc);
    Ok(())
}
