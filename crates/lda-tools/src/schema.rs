use lda_common::{Error, Result};
use serde_json::Value;

/// Validate a tool argument object against the small JSON Schema subset we emit.
///
/// Supported: object, string, integer, boolean, enum, required, maxLength,
/// minimum, maximum, additionalProperties false.
pub fn validate_arguments(schema: &Value, value: &Value) -> Result<()> {
    validate_value(schema, value, "$")
}

fn validate_value(schema: &Value, value: &Value, path: &str) -> Result<()> {
    if let Some(enum_values) = schema.get("enum").and_then(Value::as_array) {
        if !enum_values.iter().any(|item| item == value) {
            return Err(Error::InvalidInput(format!(
                "{path} is not an allowed value"
            )));
        }
    }
    match schema
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("object")
    {
        "object" => validate_object(schema, value, path),
        "string" => validate_string(schema, value, path),
        "integer" => validate_integer(schema, value, path),
        "boolean" => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(Error::InvalidInput(format!("{path} must be a boolean")))
            }
        }
        other => Err(Error::InvalidInput(format!(
            "unsupported schema type {other}"
        ))),
    }
}

fn validate_object(schema: &Value, value: &Value, path: &str) -> Result<()> {
    let Some(object) = value.as_object() else {
        return Err(Error::InvalidInput(format!("{path} must be an object")));
    };
    let properties = schema.get("properties").and_then(Value::as_object);
    let additional = schema
        .get("additionalProperties")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !additional {
        if let Some(properties) = properties {
            for key in object.keys() {
                if !properties.contains_key(key) {
                    return Err(Error::InvalidInput(format!("{path}.{key} is not allowed")));
                }
            }
        } else if !object.is_empty() {
            return Err(Error::InvalidInput(format!(
                "{path} does not accept fields"
            )));
        }
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for item in required {
            let Some(name) = item.as_str() else {
                continue;
            };
            if !object.contains_key(name) {
                return Err(Error::InvalidInput(format!("{path}.{name} is required")));
            }
        }
    }
    if let Some(properties) = properties {
        for (key, child_schema) in properties {
            if let Some(child) = object.get(key) {
                validate_value(child_schema, child, &format!("{path}.{key}"))?;
            }
        }
    }
    Ok(())
}

fn validate_string(schema: &Value, value: &Value, path: &str) -> Result<()> {
    let Some(text) = value.as_str() else {
        return Err(Error::InvalidInput(format!("{path} must be a string")));
    };
    if let Some(max) = schema.get("maxLength").and_then(Value::as_u64) {
        if text.chars().count() > max as usize {
            return Err(Error::InvalidInput(format!("{path} is too long")));
        }
    }
    Ok(())
}

fn validate_integer(schema: &Value, value: &Value, path: &str) -> Result<()> {
    let Some(number) = as_integer(value) else {
        return Err(Error::InvalidInput(format!("{path} must be an integer")));
    };
    if let Some(min) = schema.get("minimum").and_then(Value::as_i64) {
        if number < min {
            return Err(Error::InvalidInput(format!("{path} is below the minimum")));
        }
    }
    if let Some(max) = schema.get("maximum").and_then(Value::as_i64) {
        if number > max {
            return Err(Error::InvalidInput(format!("{path} is above the maximum")));
        }
    }
    Ok(())
}

fn as_integer(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    let number = value.as_f64()?;
    if number.fract() != 0.0 {
        return None;
    }
    Some(number as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["name"],
            "properties": {
                "name": {"type": "string", "maxLength": 8},
                "pid": {"type": "integer", "minimum": 1, "maximum": 100},
                "scope": {"type": "string", "enum": ["user", "system"]}
            }
        })
    }

    #[test]
    fn accepts_a_matching_object() {
        validate_arguments(
            &schema(),
            &json!({"name": "bash", "pid": 7, "scope": "user"}),
        )
        .unwrap();
    }

    #[test]
    fn rejects_extra_fields_wrong_types_and_ranges() {
        assert!(validate_arguments(&schema(), &json!({"name": "bash", "cmd": "rm"})).is_err());
        assert!(validate_arguments(&schema(), &json!({})).is_err());
        assert!(validate_arguments(&schema(), &json!({"name": "tool", "pid": 0})).is_err());
        assert!(validate_arguments(&schema(), &json!({"name": "tool", "pid": 1.5})).is_err());
        assert!(validate_arguments(&schema(), &json!({"name": "tool", "scope": "root"})).is_err());
        assert!(validate_arguments(&schema(), &json!("bash")).is_err());
    }
}
