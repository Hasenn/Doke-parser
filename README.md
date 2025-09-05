# 🚀 DokePipe

**DokePipe** is a powerful, extensible semantic parsing pipeline for Markdown.
It transforms plain Markdown text into a structured `DokeDocument`, which can then be **validated** and converted into Godot-ready data (`GodotValue`).

Think of it as a bridge between Markdown notes and **typed, validated game-ready data**.

---

## ✨ Features

* 📝 **Frontmatter Extraction** – Parse YAML frontmatter straight from Markdown.
* 🌳 **Semantic Parsing** – Convert Markdown AST into a tree of customizable `DokeNode`s.
* 🔌 **Extensible Pipeline** – Add your own parsers to interpret and transform nodes.
* 🧠 **Hypothesis System** – Compete multiple interpretations with confidence scoring.
* 🎮 **Godot Integration** – Output `GodotValue`s for GDNative/GDExtension.
* ✅ **Validation Layer** – Ensure documents are well-formed and structurally sound.

---

## 📦 Installation

In your `Cargo.toml`:

```toml
[dependencies]
doke = "0.1.0"
```

---

## 🚦 Quickstart

### Running a pipeline

```rust
use std::io::{self, Read};
use doke::{parsers, DokePipe, GodotValue};

fn main() -> Result<(), std::io::Error> {
    // Read stdin into a string
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    // Build the pipeline
    let pipe = DokePipe::new()
        .add(parsers::FrontmatterTemplateParser)
        .add(parsers::DebugPrinter); // 👀 Debug: prints nodes with emojis

    // Parse the input (for debugging)
    let doc = pipe.run_markdown(&input);

    // Validate & extract Godot values
    let values: Vec<GodotValue> = pipe.validate(&input).unwrap();

    dbg!(doc);
    dbg!(values);
    Ok(())
}
```

---

## 🗣 Sentence Parser

The **sentence parser** lets you define simple rules in YAML, avoiding the need to handcraft full grammars. It’s designed for **fast prototyping** and **structured natural language parsing**.

Its main use should be with the [typed sentences parser](#typed-sentences-parser)

### Example

```yaml
DamageEffect :
- "Deals {damage : int} damage to {target : Target}."
- "{damage_effect : DamageEffect}. On Hit :" : OnHitDamageEffect
- "Deals {multi : int}*{damage : int} damage to {target : Target}." : MultiDamageEffect

PrintEffect : 
- "Prints {message : String}." : f"The man said {message}"

Target :
- "allies" : 1
- "enemies" : 2
- "self" : 0
```

✔️ Pros:

* Simple, composable types.
* Supports children → component-based design.
* Easy to enforce strict writing style.
* Ready for i18n (translation keys).

❌ Limitations:

* Case-sensitive.
* Regex-based → ambiguous overlaps are not supported.
* Complex grammars still require custom parsers.

---

## 🌍 Internationalization Example

Sentence rules can double as **translation keys**:

```text
"DAMAGE_EFFECT_TXT_1" "Deals {damage} damage to {target}."
"DAMAGE_EFFECT_TXT_2" "Damages {target} for {damage}."
"DAMAGE_EFFECT_TXT_3" "Inflicts {damage} damage to {target}."
```

In Godot:

```gdscript
func describe():
    tr(tr_key).format({
        damage = self.damage,
        target = self.target
    })
```

This allows you to build entire **translation tables** automatically.


## Typed Sentences Parser
This is the main workflow when you don't want any re-compilation.
it uses [sentence parsers](#sentence-parser)

it can pull all the definitions from `.dokedef.yaml` files into a sentence parser for an abstract type,
and allows abstract types to define what children they allow and where they go.

### Example (Item.dokeconfig.yaml)
```yaml
rules:
  - for: ItemEffect
    parser: "**/*Effect.dokedef.yaml"
    children: 
      animations: [Animation]
      modifiers: [Modifier]
  
  - for: Animation
    parser: "**/*Animation.dokedef.yaml"
  
  - for: Modifier
    parser: "**/*Modifier.dokedef.yaml"
    children : Modifier
```

## Input Format
```
deal 10 fire damage
- play animation: anims/fire on target
heal 20 health  
- play animation: anims/heal on self
apply burning for 5 seconds
- play animation: anims/smoke on target
```

---

## 🛠 Writing Custom Parsers

You can implement your own semantic parsers by implementing `DokeParser`.

For example, here’s a **Hello World parser**:

```rust
#[derive(Debug)]
pub struct HelloWorldParser;

impl DokeParser for HelloWorldParser {
    fn process(&self, node: &mut DokeNode, _frontmatter: &HashMap<String, GodotValue>) {
        if !matches!(node.state, DokeNodeState::Unresolved) {
            return;
        }

        if node.statement.contains("Hello World") {
            let hypothesis = HelloWorldHypothesis::new(node.statement.clone(), 1.0);
            node.state = DokeNodeState::Hypothesis(vec![Box::new(hypothesis)]);
        }

        for child in &mut node.children {
            self.process(child, _frontmatter);
        }
    }
}
```

---

## 🧩 Architecture

* `DokePipe` – the pipeline runner.
* `DokeNode` – AST node with statement + children.
* `DokeParser` – trait for pluggable parsers.
* `Hypo` – trait for hypotheses with confidence scoring.
* `DokeOut` – trait for resolved semantic objects.
* `GodotValue` – typed values for Godot integration.

---

## 📚 Roadmap

* [x] Configurable typed sentence parser that uses a `MyType.dokeconf.yaml` which pulls all `MySubType.dokedef.yaml` in your repo
* [ ] Obsidian integration (live debugging inside notes).
* [ ] Improved helper tools for debugging & visualization.
* [ ] Automatic `.lalrpop` grammar generation from YAML types.

---

## 💡 Why Doke?

Because Markdown is great for **writing content**, but not great for **structured semantics**.
Doke bridges that gap:

* Designers stay in Markdown.
* Developers get typed, validated data.
* Games get their items, quests...

---
