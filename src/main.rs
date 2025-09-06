use doke::file_builder::ResourceBuilder; // <- import your new builder
use doke::parsers::{self, DebugPrinter};
use doke::{DokePipe, parsers::TypedSentencesParser};
use std::env;
use std::io::{self, Read};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 || args[1] != "--typed" {
        eprintln!("Usage: {} --typed <dokeconfig_file_path>", args[0]);
        std::process::exit(1);
    }

    let config_path = &args[2];
    let config_path = Path::new(config_path);

    // Read entire stdin into a string
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Load both the typed parser and the builder from the same config file
    let typed_parser = TypedSentencesParser::from_config_file(config_path)?;
    let file_builder = ResourceBuilder::from_file(config_path)?;

    // Build the pipeline
    let pipe = DokePipe::new()
        .add(parsers::FrontmatterTemplateParser)
        .add(typed_parser)
        .add(DebugPrinter);

    // Get the godot values from the document
    match pipe.validate(&input) {
        Err(e) => {
            eprint!("{}", e);
        }
        Ok(values) => {
            // Build the final file resource using the builder
            match file_builder.build_file_resource(values) {
                Ok(resource) => {
                    dbg!(resource);
                }
                Err(e) => {
                    eprintln!("Build error: {}", e);
                }
            }
        }
    }

    Ok(())
}
