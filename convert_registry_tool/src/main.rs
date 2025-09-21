use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs;
use toml::Table;

#[derive(Debug)]
struct Registry {
    tools: BTreeMap<String, JsonValue>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read the original flat TOML file
    let content = fs::read_to_string("../registry.toml")?;
    let parsed: Table = content.parse()?;

    let tools_table = parsed
        .get("tools")
        .ok_or("No 'tools' section found")?
        .as_table()
        .ok_or("'tools' is not a table")?;

    // When TOML parser reads flat format like "1password.aliases",
    // it automatically creates nested tables
    let mut registry_tools = BTreeMap::new();

    for (tool_name, tool_value) in tools_table {
        // The TOML parser has already converted flat keys to nested tables
        // Convert each tool's table to JSON to preserve exact structure and order
        if let Some(table) = tool_value.as_table() {
            let json_str = serde_json::to_string(table)?;
            let json_value: JsonValue = serde_json::from_str(&json_str)?;
            registry_tools.insert(tool_name.clone(), json_value);
        }
    }

    let registry = Registry {
        tools: registry_tools,
    };

    // Generate JSON output for comparison
    let json_output = serde_json::to_string_pretty(&serde_json::json!({
        "tools": registry.tools
    }))?;
    fs::write("../registry_converted.json", format!("{}\n", json_output))?;

    // Generate nested TOML output
    let mut toml_output = String::new();
    toml_output.push_str("# this file contains all the shorthand names for tools in mise\n");
    toml_output.push_str("# the format is as follows:\n");
    toml_output.push_str("# [tool-name] = [long-names...]\n");
    toml_output.push_str("# multiple are allowed for each tool because some backends may be disabled, like on windows we don't use asdf, for example\n");
    toml_output.push_str("# or a backend may be disabled via MISE_DISABLE_BACKENDS=ubi\n");
    toml_output.push('\n');

    // Convert the registry tools back to TOML with nested format
    let mut toml_table = Table::new();
    let mut tools_table = Table::new();

    for (tool_name, tool_json) in &registry.tools {
        // Convert JSON back to TOML value
        let toml_str = toml::to_string(&tool_json)?;
        let tool_value: toml::Value = toml_str.parse()?;
        tools_table.insert(tool_name.clone(), tool_value);
    }

    toml_table.insert("tools".to_string(), toml::Value::Table(tools_table));

    // Serialize to TOML string
    let toml_string = toml::to_string_pretty(&toml_table)?;
    toml_output.push_str(&toml_string);

    fs::write("../registry_nested.toml", toml_output)?;

    println!("Conversion completed successfully!");
    println!("Generated: registry_nested.toml");
    println!("Generated: registry_converted.json");

    // Verify JSON output matches the reference
    let reference_json = fs::read_to_string("../registry_original.json")?;
    let new_json = fs::read_to_string("../registry_converted.json")?;

    // Parse both JSONs to compare structure (ignoring formatting)
    let ref_value: JsonValue = serde_json::from_str(&reference_json)?;
    let new_value: JsonValue = serde_json::from_str(&new_json)?;

    if ref_value == new_value {
        println!("✅ JSON outputs are functionally identical!");
    } else {
        println!("⚠️  JSON outputs differ functionally");

        // Try to find first difference for debugging
        if let (Some(ref_tools), Some(new_tools)) = (ref_value.get("tools"), new_value.get("tools"))
        {
            if let (Some(ref_obj), Some(new_obj)) = (ref_tools.as_object(), new_tools.as_object()) {
                for (key, ref_val) in ref_obj {
                    if let Some(new_val) = new_obj.get(key) {
                        if ref_val != new_val {
                            println!("First difference found in tool: {}", key);
                            break;
                        }
                    } else {
                        println!("Tool {} missing in new JSON", key);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
