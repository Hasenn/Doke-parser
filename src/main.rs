use doke::parsers;
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

    // Read entire stdin into a string
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Load the typed sentences parser from configuration file
    let typed_parser = TypedSentencesParser::from_config_file(Path::new(config_path))?;

    // Build the pipeline
    let pipe = DokePipe::new()
        .add(parsers::FrontmatterTemplateParser)
        .add(typed_parser) // Use the typed sentences parser
        .add(parsers::DebugPrinter) // prints nodes with emojis for debugging
    ;
    dbg!(&pipe);

    // Get the godot values from the document
    let _res = pipe.validate(&input);
    match _res {
        Err(e) => {
            eprint!("{}", e);
        }
        Ok(val) => {
            dbg!(val);
        }
    }
    Ok(())
}
