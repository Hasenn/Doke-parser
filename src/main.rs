use doke::{DokePipe, GodotValue, parsers};
use std::io::{self, Read};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read entire stdin into a string
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let sentence_config = r#"
DamageEffect:
  - "Deals {damage: int} damage"
  - "Deals {damage: int} damage to {target : Target}"
HealEffect:
  - "Heals for {amount: int}"
  - "Heals {target : Target} for {amount : int}"
Target:
  - "allies" : 1
  - "enemies" : 2
  - "self" : 3
ReactionEffect:
  - "When hit : {damage_effect: DamageEffect}"
ComboEffect:
  - "First: {dmg: DamageEffect}, Then: {heal: HealEffect}, Reaction: {reaction: ReactionEffect}"
"#;
    let sentence_parser = parsers::SentenceParser::from_yaml(sentence_config)?;

    // Build the pipeline with DebugPrinter included
    let pipe = DokePipe::new()
        .add(parsers::FrontmatterTemplateParser)
        .map(sentence_parser)
        .add(parsers::DebugPrinter) // prints nodes with emojis for debugging
    ;
    // Get the godot values from the document
    let _val: Vec<GodotValue> = pipe.validate(&input)?;
    dbg!(_val);
    Ok(())
}
