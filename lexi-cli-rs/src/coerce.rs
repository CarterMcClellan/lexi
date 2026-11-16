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
}
