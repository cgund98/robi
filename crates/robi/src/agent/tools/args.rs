//! Turn a tool-call object into its argument struct.
//!
//! A parse failure names the keys that arrived and the required keys that did
//! not. Values stay out of the message: several of them are file text.

use robi_core::error::ToolError;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(super) fn parse_args<T: DeserializeOwned>(
    args: Value,
    required: &[&str],
) -> Result<T, ToolError> {
    serde_json::from_value(args.clone()).map_err(|err| {
        ToolError::InvalidArgs(format!(
            "parse arguments: {err}. {}",
            argument_names(&args, required)
        ))
    })
}

fn argument_names(args: &Value, required: &[&str]) -> String {
    let Some(object) = args.as_object() else {
        return required_clause(required);
    };
    let received = if object.is_empty() {
        "none".to_string()
    } else {
        object.keys().cloned().collect::<Vec<_>>().join(", ")
    };
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|name| !object.contains_key(*name))
        .collect();
    let received = format!("Received {received}.");
    if missing.is_empty() {
        if required.is_empty() {
            received
        } else {
            format!("{received} {}", required_clause(required))
        }
    } else {
        format!(
            "{received} Missing {}. {}",
            missing.join(", "),
            required_clause(required)
        )
    }
}

fn required_clause(required: &[&str]) -> String {
    if required.is_empty() {
        "Required: none.".to_string()
    } else {
        format!("Required: {}.", required.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Sample {
        path: String,
        old_string: String,
        new_string: String,
    }

    #[test]
    fn a_missing_key_names_what_arrived_and_what_is_required() {
        let Err(error) = parse_args::<Sample>(
            serde_json::json!({"path": "a.txt", "old_string": "gone", "new": "kept"}),
            &["path", "old_string", "new_string"],
        ) else {
            panic!("expected a missing field");
        };
        assert_eq!(
            error.to_string(),
            "invalid arguments: parse arguments: missing field `new_string`. \
             Received path, old_string, new. Missing new_string. Required: path, old_string, new_string."
        );
    }

    #[test]
    fn an_empty_string_is_present() {
        let Ok(args) = parse_args::<Sample>(
            serde_json::json!({"path": "a.txt", "old_string": "gone", "new_string": ""}),
            &["path", "old_string", "new_string"],
        ) else {
            panic!("an empty string is a present field");
        };
        assert_eq!(args.path, "a.txt");
        assert_eq!(args.old_string, "gone");
        assert_eq!(args.new_string, "");
    }

    #[test]
    fn a_non_object_names_the_required_keys() {
        let Err(error) = parse_args::<Sample>(serde_json::json!([]), &["path"]) else {
            panic!("expected a type error");
        };
        assert!(error.to_string().ends_with("Required: path."), "{error}");
    }
}
