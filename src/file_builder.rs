use crate::semantic::GodotValue;
use std::{collections::{HashMap, HashSet}, fmt::format, fs, path::Path};
use hashlink::LinkedHashMap;
use thiserror::Error;
use yaml_rust2::{Yaml, YamlLoader};

#[derive(Debug, Error)]
pub enum BuilderError {
    #[error("YAML parse error: {0}")]
    Yaml(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid Config: {0}")]
    Config(String),

    #[error("Missing required field '{0}' of type '{1}'")]
    MissingField(String, String),

    #[error("Type mismatch for field '{0}': expected {1}, got {2}")]
    TypeMismatch(String, String, String),
}

/// Normalized config after parsing/validation
#[derive(Debug, Clone)]
pub struct Config {
    pub root: String,
    pub children: Vec<FieldConfig>,
}

#[derive(Debug, Clone)]
pub struct FieldConfig {
    pub name: String,
    pub ty: FieldType,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub enum FieldType {
    Single(String), // "ItemAction", "String", "int"
    Array(String),  // "[ItemModifier]", "[String]"
}

#[derive(Debug)]
pub struct ResourceBuilder {
    config: Config,
}

impl ResourceBuilder {
    pub fn from_config(config: Config) -> Result<Self, BuilderError> {
        dbg!(&config);
        // Validate ? ordering
        let mut seen_optional: HashSet<&String> = HashSet::new();
        for field in &config.children {
            let ty_name = match &field.ty {
                FieldType::Single(t) => t,
                FieldType::Array(t) => t,
            };
            match &field.optional {
                true => {
                    seen_optional.insert(ty_name);
                },
                false => {
                    // if a required field for a type comes after a required one, config is invalid !
                    if seen_optional.contains(&ty_name) {
                        return Err(BuilderError::Config(format!("An optional {} came before a required one in {} : \n", &ty_name, field.name)))
                    }
                }
            }
        }

        Ok(Self { config })
    }

    pub fn from_file(path: &Path) -> Result<Self, BuilderError> {
        let s = fs::read_to_string(path)?;
        let docs = YamlLoader::load_from_str(&s).map_err(|e| BuilderError::Yaml(e.to_string()))?;
        let yaml = docs
            .into_iter()
            .next()
            .ok_or_else(|| BuilderError::Yaml("Empty YAML file".into()))?;

        let config = Self::parse_config(&yaml)?;
        Self::from_config(config)
    }
    fn parse_config(y: &Yaml) -> Result<Config, BuilderError> {
        // root
        let root_yaml = y["root"]
            .as_str()
            .ok_or_else(|| BuilderError::Config("Missing 'root' string key".into()))?;
        let root = root_yaml.to_string();

        // children
        let children_yaml = y["children"].as_vec().ok_or_else(|| {
            BuilderError::Config("Missing or invalid 'children' (must be a sequence)".into())
        })?;

        let mut children = Vec::new();

        for entry in children_yaml {
            let obj = entry
                .as_hash()
                .ok_or_else(|| BuilderError::Config("Each child must be a map".into()))?;

            if obj.len() != 1 {
                return Err(BuilderError::Config(format!(
                    "Each child must have exactly one key, got {:?}",
                    obj
                )));
            }

            let (raw_name, value) = obj.iter().next().unwrap();
            let mut name = raw_name
                .as_str()
                .ok_or_else(|| BuilderError::Config("Child field name must be string".into()))?
                .to_string();

            let mut optional = false;
            if name.ends_with('?') {
                optional = true;
                name.pop();
            }

            let ty = if let Some(s) = value.as_str() {
                FieldType::Single(s.to_string())
            } else if let Some(arr) = value.as_vec() {
                if arr.len() != 1 {
                    return Err(BuilderError::Config(format!(
                        "Array field {} must have exactly one type, got {:?}",
                        name, arr
                    )));
                }
                let s = arr[0]
                    .as_str()
                    .ok_or_else(|| BuilderError::Config("Array element must be string".into()))?;
                FieldType::Array(s.to_string())
            } else {
                return Err(BuilderError::Config(format!(
                    "Invalid type spec for field {}",
                    name
                )));
            };

            children.push(FieldConfig { name, ty, optional });
        }

        Ok(Config { root, children })
    }
    pub fn build_file_resource(&self, values: Vec<GodotValue>) -> Result<GodotValue, BuilderError> {
        let mut fields: HashMap<String, GodotValue> = HashMap::new();
        let mut unused = values;

        for fc in &self.config.children {
            match &fc.ty {
                FieldType::Array(ty) => {
                    let mut collected = Vec::new();
                    let mut keep = Vec::new();
                    for v in unused {
                        if matches_type(&v, ty) {
                            collected.push(v);
                        } else {
                            keep.push(v);
                        }
                    }
                    unused = keep;

                    if !collected.is_empty() {
                        fields.insert(fc.name.clone(), GodotValue::Array(collected));
                    } else if fc.optional {
                        // Optional arrays default to empty
                        fields.insert(fc.name.clone(), GodotValue::Array(vec![]));
                    } else {
                        return Err(BuilderError::MissingField(fc.name.clone(), ty.clone()));
                    }
                }
                FieldType::Single(ty) => {
                    let mut found_idx = None;
                    for (i, v) in unused.iter().enumerate() {
                        if matches_type(v, ty) {
                            found_idx = Some(i);
                            break;
                        }
                    }

                    if let Some(idx) = found_idx {
                        let v = unused.remove(idx);
                        fields.insert(fc.name.clone(), v);
                    } else if fc.optional {
                        // Optional singletons default to Nil
                        fields.insert(fc.name.clone(), GodotValue::Nil);
                    } else {
                        return Err(BuilderError::MissingField(fc.name.clone(), ty.clone()));
                    }
                }
            }
        }
        Ok(GodotValue::Resource {
            type_name: self.config.root.clone(),
            abstract_type_name: "root".to_string(),
            fields : fields,
        })
    }
}
/// Helper: check whether a GodotValue matches the expected type name
fn matches_type(v: &GodotValue, ty: &str) -> bool {
    match v {
        GodotValue::Int(_) => ty.eq_ignore_ascii_case("int"),
        GodotValue::Float(_) => ty.eq_ignore_ascii_case("float"),
        GodotValue::String(_) => ty.eq_ignore_ascii_case("string"),
        GodotValue::Array(_) => ty.eq_ignore_ascii_case("array"),
        GodotValue::Dict(_) => ty.eq_ignore_ascii_case("dict"),
        GodotValue::Bool(_) => ty.eq_ignore_ascii_case("bool"),
        GodotValue::Resource { type_name, abstract_type_name, .. } => {
            type_name == ty || abstract_type_name == ty
        }
        GodotValue::Nil => ty.eq_ignore_ascii_case("nil"),
    }
}

