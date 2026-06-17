//! JSON Schema validation for tool arguments.
//!
//! Every [`ToolDefinition`](crate::tool::ToolDefinition) carries an `input_schema`
//! field. This module compiles and evaluates that schema against the raw
//! arguments the model sends, producing structured validation errors.

use serde_json::Value;

use crate::error::AgentError;

/// A single validation failure with a JSON-Pointer-style path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Human-readable description of what failed.
    pub message: String,
    /// JSON Pointer to the offending field (e.g. `/required/0`).
    pub path: String,
}

/// Result of validating tool arguments against a JSON Schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    /// `true` when the arguments conform to the schema.
    pub valid: bool,
    /// Structured errors when `valid` is `false`.
    pub errors: Vec<ValidationError>,
}

/// Validate `arguments` against `schema`.
///
/// Returns a structured result rather than raising, so callers can decide
/// whether to surface the errors to the model as a recoverable tool result.
pub fn validate_arguments(schema: &Value, arguments: &Value) -> ValidationResult {
    match jsonschema::validator_for(schema) {
        Ok(validator) => {
            let mut errors = Vec::new();
            for err in validator.iter_errors(arguments) {
                errors.push(ValidationError {
                    message: err.to_string(),
                    path: err.instance_path.to_string(),
                });
            }
            ValidationResult {
                valid: errors.is_empty(),
                errors,
            }
        }
        Err(err) => ValidationResult {
            valid: false,
            errors: vec![ValidationError {
                message: format!("invalid tool input schema: {err}"),
                path: String::new(),
            }],
        },
    }
}

/// Convenience: validate and return an [`AgentError::Validation`] if invalid.
pub fn validate_arguments_or_error(
    tool_name: &str,
    schema: &Value,
    arguments: &Value,
) -> Result<(), AgentError> {
    let result = validate_arguments(schema, arguments);
    if result.valid {
        Ok(())
    } else {
        let details: Vec<String> = result
            .errors
            .iter()
            .map(|e| format!("{}: {}", e.path, e.message))
            .collect();
        Err(AgentError::Validation(format!(
            "tool `{tool_name}` arguments failed schema validation: {}",
            details.join("; ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn simple_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "count": { "type": "integer" }
            },
            "required": ["name"]
        })
    }

    #[test]
    fn accepts_valid_arguments() {
        let result = validate_arguments(&simple_schema(), &json!({"name": "test", "count": 3}));
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn rejects_missing_required_field() {
        let result = validate_arguments(&simple_schema(), &json!({"count": 3}));
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn rejects_wrong_type() {
        let result =
            validate_arguments(&simple_schema(), &json!({"name": "test", "count": "three"}));
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn rejects_invalid_schema() {
        let result = validate_arguments(&json!({"type": "invalid_type"}), &json!({}));
        assert!(!result.valid);
    }
}
