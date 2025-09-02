use std::io::{self, Read};
use doke_parser::{parsers, DokePipe};

fn main() -> Result<(), std::io::Error> {
    // Read entire stdin into a string
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Build the pipeline with DebugPrinter included
    let pipe = DokePipe::new()
        .add(parsers::FrontmatterTemplateParser)
        .add(parsers::DebugPrinter); // prints nodes with emojis

    // Run the pipeline on the input Markdown
    let _doc = pipe.run_markdown(&input);
    dbg!(_doc);
    Ok(())
}
