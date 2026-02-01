//! Build script to generate schema data from mise.json

use serde_json::Value;
use std::env;
use std::fs;
use std::path::Path;

/// Schema type information for a property
#[derive(Debug, Clone, PartialEq)]
enum SchemaType {
    String,
    Boolean,
    Integer,
    Number,
    Array,
    Object,
    Unknown,
}

impl SchemaType {
    fn as_str(&self) -> &'static str {
        match self {
            SchemaType::String => "string",
            SchemaType::Boolean => "boolean",
            SchemaType::Integer => "integer",
            SchemaType::Number => "number",
            SchemaType::Array => "array",
            SchemaType::Object => "object",
            SchemaType::Unknown => "unknown",
        }
    }
}

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("schema_sections.rs");

    // Read the schema file - check local copy first (used by cargo publish),
    // then fall back to repo-root location (used during normal development)
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let local_schema = manifest_dir.join("mise.json");
    let repo_schema = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("schema/mise.json");
    let schema_path = if local_schema.exists() {
        local_schema
    } else {
        repo_schema
    };

    println!("cargo:rerun-if-changed={}", schema_path.display());

    let schema_content = fs::read_to_string(&schema_path)
        .unwrap_or_else(|e| panic!("Failed to read schema at {}: {}", schema_path.display(), e));

    let schema: Value = serde_json::from_str(&schema_content)
        .unwrap_or_else(|e| panic!("Failed to parse schema JSON: {}", e));

    let defs = schema.get("$defs");

    // Extract top-level properties and classify them
    let mut sections = Vec::new();
    let mut entries = Vec::new(); // (name, description, type)

    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
        for (name, prop) in properties {
            // Skip deprecated and internal properties
            if prop
                .get("deprecated")
                .and_then(|d| d.as_bool())
                .unwrap_or(false)
            {
                continue;
            }
            if name == "_" {
                continue;
            }

            let description = prop
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");

            let is_section = is_section_property(prop, defs);

            if is_section {
                sections.push((name.clone(), description.to_string()));
            } else {
                let schema_type = get_schema_type(prop, defs);
                entries.push((name.clone(), description.to_string(), schema_type));
            }
        }
    }

    // Extract settings keys from $defs/settings (with type info)
    let mut settings_keys = Vec::new(); // (name, description, type)
    if let Some(settings_def) = defs.and_then(|d| d.get("settings")) {
        extract_settings_keys(settings_def, defs, "", &mut settings_keys);
    }

    // Extract task_config keys from $defs/task_config
    let mut task_config_keys = Vec::new();
    if let Some(task_config_def) = defs.and_then(|d| d.get("task_config")) {
        extract_simple_properties(task_config_def, defs, &mut task_config_keys);
    }

    // Extract monorepo keys from $defs/monorepo
    let mut monorepo_keys = Vec::new();
    if let Some(monorepo_def) = defs.and_then(|d| d.get("monorepo")) {
        extract_simple_properties(monorepo_def, defs, &mut monorepo_keys);
    }

    // Sort by name for consistent output
    sections.sort_by(|a, b| a.0.cmp(&b.0));
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    settings_keys.sort_by(|a, b| a.0.cmp(&b.0));
    task_config_keys.sort_by(|a, b| a.0.cmp(&b.0));
    monorepo_keys.sort_by(|a, b| a.0.cmp(&b.0));

    // Generate the Rust code
    let mut code = String::new();

    // Generate SchemaType enum
    code.push_str("/// Type of a schema property\n");
    code.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    code.push_str("pub enum SchemaType {\n");
    code.push_str("    String,\n");
    code.push_str("    Boolean,\n");
    code.push_str("    Integer,\n");
    code.push_str("    Number,\n");
    code.push_str("    Array,\n");
    code.push_str("    Object,\n");
    code.push_str("    Unknown,\n");
    code.push_str("}\n\n");

    // Generate sections constant
    code.push_str("/// Valid top-level sections in mise.toml (tables with user-defined keys)\n");
    code.push_str("pub const SCHEMA_SECTIONS: &[(&str, &str)] = &[\n");
    for (name, description) in &sections {
        let escaped_desc = escape_string(description);
        code.push_str(&format!("    (\"{}\", \"{}\"),\n", name, escaped_desc));
    }
    code.push_str("];\n\n");

    // Generate entries constant with type info
    code.push_str(
        "/// Valid top-level entries in mise.toml (scalar values, not sections) with type info\n",
    );
    code.push_str("pub const SCHEMA_ENTRIES: &[(&str, &str, SchemaType)] = &[\n");
    for (name, description, schema_type) in &entries {
        let escaped_desc = escape_string(description);
        code.push_str(&format!(
            "    (\"{}\", \"{}\", SchemaType::{}),\n",
            name,
            escaped_desc,
            capitalize_first(schema_type.as_str())
        ));
    }
    code.push_str("];\n\n");

    // Generate settings keys constant with type info
    code.push_str("/// Valid settings keys in mise.toml [settings] section with type info\n");
    code.push_str("pub const SCHEMA_SETTINGS: &[(&str, &str, SchemaType)] = &[\n");
    for (name, description, schema_type) in &settings_keys {
        let escaped_desc = escape_string(description);
        code.push_str(&format!(
            "    (\"{}\", \"{}\", SchemaType::{}),\n",
            name,
            escaped_desc,
            capitalize_first(schema_type.as_str())
        ));
    }
    code.push_str("];\n\n");

    // Generate common hooks constant
    code.push_str("/// Common hook names in mise.toml [hooks] section\n");
    code.push_str("pub const SCHEMA_HOOKS: &[(&str, &str)] = &[\n");
    code.push_str("    (\"enter\", \"Run when entering a directory with this mise.toml\"),\n");
    code.push_str("    (\"leave\", \"Run when leaving a directory with this mise.toml\"),\n");
    code.push_str("    (\"cd\", \"Run on any directory change\"),\n");
    code.push_str("    (\"preinstall\", \"Run before installing a tool\"),\n");
    code.push_str("    (\"postinstall\", \"Run after installing a tool\"),\n");
    code.push_str("];\n\n");

    // Generate task_config keys constant with type info
    code.push_str("/// Valid keys in mise.toml [task_config] section with type info\n");
    code.push_str("pub const SCHEMA_TASK_CONFIG: &[(&str, &str, SchemaType)] = &[\n");
    for (name, description, schema_type) in &task_config_keys {
        let escaped_desc = escape_string(description);
        code.push_str(&format!(
            "    (\"{}\", \"{}\", SchemaType::{}),\n",
            name,
            escaped_desc,
            capitalize_first(schema_type.as_str())
        ));
    }
    code.push_str("];\n\n");

    // Generate monorepo keys constant with type info
    code.push_str("/// Valid keys in mise.toml [monorepo] section with type info\n");
    code.push_str("pub const SCHEMA_MONOREPO: &[(&str, &str, SchemaType)] = &[\n");
    for (name, description, schema_type) in &monorepo_keys {
        let escaped_desc = escape_string(description);
        code.push_str(&format!(
            "    (\"{}\", \"{}\", SchemaType::{}),\n",
            name,
            escaped_desc,
            capitalize_first(schema_type.as_str())
        ));
    }
    code.push_str("];\n");

    fs::write(&dest_path, code).unwrap();
}

/// Capitalize first letter
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Get the schema type for a property
fn get_schema_type(prop: &Value, defs: Option<&Value>) -> SchemaType {
    // Check direct type first
    if let Some(type_val) = prop.get("type").and_then(|t| t.as_str()) {
        return match type_val {
            "string" => SchemaType::String,
            "boolean" => SchemaType::Boolean,
            "integer" => SchemaType::Integer,
            "number" => SchemaType::Number,
            "array" => SchemaType::Array,
            "object" => SchemaType::Object,
            _ => SchemaType::Unknown,
        };
    }

    // Handle $ref
    if let Some(ref_val) = prop.get("$ref").and_then(|r| r.as_str())
        && let Some(def_name) = ref_val.strip_prefix("#/$defs/")
        && let Some(def) = defs.and_then(|d| d.get(def_name))
    {
        return get_schema_type(def, defs);
    }

    // Handle oneOf - return the first simple type found
    if let Some(one_of) = prop.get("oneOf").and_then(|o| o.as_array()) {
        for option in one_of {
            let t = get_schema_type(option, defs);
            if t != SchemaType::Unknown && t != SchemaType::Object {
                return t;
            }
        }
    }

    SchemaType::Unknown
}

/// Extract simple properties from a schema object (non-recursive) with type info
fn extract_simple_properties(
    prop: &Value,
    defs: Option<&Value>,
    keys: &mut Vec<(String, String, SchemaType)>,
) {
    if let Some(properties) = prop.get("properties").and_then(|p| p.as_object()) {
        for (name, prop_value) in properties {
            // Skip deprecated properties
            if prop_value
                .get("deprecated")
                .and_then(|d| d.as_bool())
                .unwrap_or(false)
            {
                continue;
            }

            let description = prop_value
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");

            let schema_type = get_schema_type(prop_value, defs);
            keys.push((name.clone(), description.to_string(), schema_type));
        }
    }
}

/// Extract settings keys recursively, with dot notation for nested settings and type info
fn extract_settings_keys(
    prop: &Value,
    defs: Option<&Value>,
    prefix: &str,
    keys: &mut Vec<(String, String, SchemaType)>,
) {
    if let Some(properties) = prop.get("properties").and_then(|p| p.as_object()) {
        for (name, prop_value) in properties {
            // Skip deprecated properties
            if prop_value
                .get("deprecated")
                .and_then(|d| d.as_bool())
                .unwrap_or(false)
            {
                continue;
            }

            let full_name = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}.{}", prefix, name)
            };

            let description = prop_value
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");

            // Check if this is a nested object with properties (like aqua, cargo, etc.)
            let is_nested_object = prop_value.get("type").and_then(|t| t.as_str())
                == Some("object")
                && prop_value.get("properties").is_some()
                && prop_value
                    .get("additionalProperties")
                    .and_then(|a| a.as_bool())
                    == Some(false);

            if is_nested_object {
                // Recurse into nested settings
                extract_settings_keys(prop_value, defs, &full_name, keys);
            } else {
                // Add this as a leaf setting with type
                let schema_type = get_schema_type(prop_value, defs);
                keys.push((full_name, description.to_string(), schema_type));
            }
        }
    }
}

/// Escape special characters in strings for Rust string literals
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Determine if a property represents a TOML section (table with user-defined keys)
/// vs a simple entry (scalar, array, or fixed-structure object)
fn is_section_property(prop: &Value, defs: Option<&Value>) -> bool {
    // Check if it directly has additionalProperties (user-defined keys)
    if prop.get("additionalProperties").is_some() {
        return true;
    }

    // Check the type
    if let Some(type_val) = prop.get("type").and_then(|t| t.as_str()) {
        match type_val {
            "array" => return false, // Arrays are entries, not sections
            "string" | "number" | "boolean" | "integer" => return false, // Scalars
            "object" => {
                // Object type - check if it has additionalProperties or is just fixed properties
                if prop.get("additionalProperties").is_some() {
                    return true;
                }
                // Check if this is a fixed-structure object (like min_version with hard/soft)
                // If it only has "properties" without additionalProperties, treat as entry
                if prop.get("properties").is_some() && prop.get("additionalProperties").is_none() {
                    return false;
                }
                // Default to section for plain objects
                return true;
            }
            _ => {}
        }
    }

    // Handle $ref - look up the definition
    if let Some(ref_val) = prop.get("$ref").and_then(|r| r.as_str())
        && let Some(def_name) = ref_val.strip_prefix("#/$defs/")
        && let Some(def) = defs.and_then(|d| d.get(def_name))
    {
        return is_section_property(def, defs);
    }

    // Handle oneOf - if any option is a simple type, treat as entry
    if let Some(one_of) = prop.get("oneOf").and_then(|o| o.as_array()) {
        // If oneOf includes a simple type (string, number), it's an entry
        for option in one_of {
            if let Some(type_val) = option.get("type").and_then(|t| t.as_str())
                && matches!(type_val, "string" | "number" | "boolean" | "integer")
            {
                return false;
            }
        }
        // If all options are objects/refs, check if any has additionalProperties
        for option in one_of {
            if is_section_property(option, defs) {
                return true;
            }
        }
        return false;
    }

    // Default to section (most mise.toml properties are sections)
    true
}
