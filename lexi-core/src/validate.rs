//! JSON-Schema subset validation.
//!
//! Mirrors `lexi_cli.api_compose.validate`: type, const, enum, min/max,
//! required, properties, items, oneOf-by-successful-subvalidation. Error
//! message text is not part of the contract (only pass/fail matters).

use serde_json::Value;

fn matches_type(value: &Value, type_name: &str) -> bool {
    match type_name {
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => true, // unknown type: lenient
    }
}

/// Return validation errors for `instance` against `schema` (empty = valid).
pub fn validate(schema: &Value, instance: &Value, path: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let schema = match schema.as_object() {
        Some(s) => s,
        None => return errors,
    };

    if let Some(types) = schema.get("type") {
        let type_list: Vec<String> = match types {
            Value::String(s) => vec![s.clone()],
            Value::Array(a) => a
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };
        if !type_list.is_empty() && !type_list.iter().any(|t| matches_type(instance, t)) {
            errors.push(format!("{}: type mismatch {:?}", loc(path), type_list));
            return errors;
        }
    }

    if let Some(c) = schema.get("const") {
        if instance != c {
            errors.push(format!("{}: const mismatch", loc(path)));
        }
    }

    if let Some(Value::Array(choices)) = schema.get("enum") {
        if !choices.iter().any(|c| c == instance) {
            errors.push(format!("{}: not in enum", loc(path)));
        }
    }

    if instance.is_number() {
        let n = instance.as_f64().unwrap();
        if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
            if n < min {
                errors.push(format!("{}: < minimum", loc(path)));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
            if n > max {
                errors.push(format!("{}: > maximum", loc(path)));
            }
        }
    }

    if let Value::Object(obj) = instance {
        if schema.contains_key("properties") || schema.contains_key("required") {
            if let Some(Value::Array(req)) = schema.get("required") {
                for key in req.iter().filter_map(Value::as_str) {
                    if !obj.contains_key(key) {
                        errors.push(format!("{}: missing required {:?}", loc(path), key));
                    }
                }
            }
            if let Some(Value::Object(props)) = schema.get("properties") {
                for (key, val) in obj {
                    if let Some(child_schema) = props.get(key) {
                        let child_path = if path.is_empty() {
                            key.clone()
                        } else {
                            format!("{path}.{key}")
                        };
                        errors.extend(validate(child_schema, val, &child_path));
                    }
                }
            }
        }
    }

    if let Value::Array(items) = instance {
        if let Some(item_schema) = schema.get("items") {
            for (i, item) in items.iter().enumerate() {
                errors.extend(validate(item_schema, item, &format!("{path}[{i}]")));
            }
        }
    }

    if let Some(Value::Array(alts)) = schema.get("oneOf") {
        let matches: Vec<usize> = alts
            .iter()
            .enumerate()
            .filter(|(_, alt)| validate(alt, instance, path).is_empty())
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            errors.push(format!("{}: no oneOf match", loc(path)));
        } else if matches.len() > 1 {
            errors.push(format!("{}: multiple oneOf matches", loc(path)));
        }
    }

    errors
}

fn loc(path: &str) -> &str {
    if path.is_empty() {
        "value"
    } else {
        path
    }
}
