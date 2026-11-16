//! `--set path=value` parsing, shared by `prompts set` and (later) `respond`.

use serde_json::Value;

/// Parse `path=value` into the path and a type-coerced value.
pub(crate) fn parse_set_arg(raw: &str) -> Result<(String, Value), String> {
    let Some((path, value)) = raw.split_once('=') else {
        return Err(format!("--set requires path=value (got {raw:?})"));
    };
    let path = path.trim();
    if path.is_empty() {
        return Err(format!("--set path is empty in {raw:?}"));
    }
    Ok((path.to_string(), coerce_value(value)?))
}

/// Coerce a `--set` value (same rules as the Python CLI):
/// `:json:<expr>` forces JSON; `true`/`false`/`null` (or `True`/`False`/`None`)
/// become literals; integers and floats become numbers; text starting with `{`
/// or `[` is parsed as JSON; anything else stays a string.
pub(crate) fn coerce_value(raw: &str) -> Result<Value, String> {
    if let Some(expr) = raw.strip_prefix(":json:") {
        return serde_json::from_str(expr).map_err(|e| format!("invalid :json: value: {e}"));
    }
    match raw {
        "true" | "True" => return Ok(Value::Bool(true)),
        "false" | "False" => return Ok(Value::Bool(false)),
        "null" | "None" => return Ok(Value::Null),
        _ => {}
    }
    if let Ok(i) = raw.trim().parse::<i64>() {
        return Ok(Value::from(i));
    }
    if let Ok(f) = raw.trim().parse::<f64>() {
        if f.is_finite() {
            return Ok(Value::from(f));
        }
    }
    let stripped = raw.trim_start();
    if stripped.starts_with('{') || stripped.starts_with('[') {
        if let Ok(v) = serde_json::from_str(stripped) {
            return Ok(v);
        }
    }
    Ok(Value::String(raw.to_string()))
}

/// Coerce a schema flag's value to its spec's type (the Python CLI's
/// `_coerce_typed_value`): an `enum_map` entry wins; a type list tries each;
/// integer / number / boolean / object|array (JSON) are parsed; anything else
/// is a string that must be in `enum` when one is given.
pub(crate) fn coerce_typed(raw: &str, spec: &Value) -> Result<Value, String> {
    if let Some(mapped) = spec.get("enum_map").and_then(|m| m.get(raw)) {
        return Ok(mapped.clone());
    }
    coerce_to(
        raw,
        spec.get("type"),
        spec.get("enum").and_then(Value::as_array),
    )
}

fn coerce_to(
    raw: &str,
    ty: Option<&Value>,
    enumeration: Option<&Vec<Value>>,
) -> Result<Value, String> {
    match ty {
        Some(Value::Array(types)) => {
            for t in types {
                if let Ok(v) = coerce_to(raw, Some(t), None) {
                    return Ok(v);
                }
            }
            Err(format!(
                "could not coerce {raw:?} to any of {}",
                Value::Array(types.clone())
            ))
        }
        Some(Value::String(t)) if t == "integer" => raw
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| format!("expected integer, got {raw:?}")),
        Some(Value::String(t)) if t == "number" => raw
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(Value::from)
            .ok_or_else(|| format!("expected number, got {raw:?}")),
        Some(Value::String(t)) if t == "boolean" => match raw {
            "true" | "True" | "1" | "yes" => Ok(Value::Bool(true)),
            "false" | "False" | "0" | "no" => Ok(Value::Bool(false)),
            _ => Err(format!("expected boolean, got {raw:?}")),
        },
        Some(Value::String(t)) if t == "object" || t == "array" => {
            serde_json::from_str(raw).map_err(|_| format!("expected JSON {t}, got {raw:?}"))
        }
        _ => {
            if let Some(allowed) = enumeration {
                if !allowed.iter().any(|a| a.as_str() == Some(raw)) {
                    return Err(format!(
                        "value {raw:?} not in enum {}",
                        Value::Array(allowed.clone())
                    ));
                }
            }
            Ok(Value::from(raw))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coerces_like_the_python_cli() {
        assert_eq!(coerce_value("3").unwrap(), json!(3));
        assert_eq!(coerce_value("0.5").unwrap(), json!(0.5));
        assert_eq!(coerce_value("True").unwrap(), json!(true));
        assert_eq!(coerce_value("null").unwrap(), Value::Null);
        assert_eq!(coerce_value("[1, 2]").unwrap(), json!([1, 2]));
        assert_eq!(coerce_value("{bad").unwrap(), json!("{bad"));
        assert_eq!(coerce_value(":json:\"7\"").unwrap(), json!("7"));
        assert_eq!(coerce_value("high").unwrap(), json!("high"));
        assert_eq!(
            parse_set_arg("reasoning.effort=high").unwrap(),
            ("reasoning.effort".into(), json!("high"))
        );
        assert!(parse_set_arg("noequals").is_err());
        assert!(parse_set_arg("=1").is_err());
    }

    #[test]
    fn coerces_flag_values_to_their_schema_type() {
        let spec = |s: Value| s;
        assert_eq!(
            coerce_typed("3", &spec(json!({"type": "integer"}))).unwrap(),
            json!(3)
        );
        assert!(coerce_typed("x", &spec(json!({"type": "integer"}))).is_err());
        assert_eq!(
            coerce_typed("0.5", &spec(json!({"type": "number"}))).unwrap(),
            json!(0.5)
        );
        assert_eq!(
            coerce_typed("yes", &spec(json!({"type": "boolean"}))).unwrap(),
            json!(true)
        );
        assert_eq!(
            coerce_typed("[1]", &spec(json!({"type": "array"}))).unwrap(),
            json!([1])
        );
        assert_eq!(
            coerce_typed("7", &spec(json!({"type": ["integer", "string"]}))).unwrap(),
            json!(7)
        );
        assert_eq!(
            coerce_typed("hi", &spec(json!({"type": ["integer", "string"]}))).unwrap(),
            json!("hi")
        );
        let e = spec(json!({"type": "string", "enum": ["low", "high"]}));
        assert_eq!(coerce_typed("low", &e).unwrap(), json!("low"));
        assert!(coerce_typed("mid", &e).is_err());
        let m = spec(json!({"type": "string", "enum_map": {"on": "auto"}}));
        assert_eq!(coerce_typed("on", &m).unwrap(), json!("auto"));
    }
}
