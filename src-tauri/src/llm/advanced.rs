//! Advanced (power-user) overrides shared by every BYOK provider path.
//!
//! The goal is to expose prompt text, token budgets, and raw request parameters
//! without adding a per-provider settings matrix. Everything here is generic:
//! a JSON object is merged into the provider request body, and prompt text is
//! appended to the prompt the app already built.
//!
//! Safety rails:
//! * A malformed or non-object JSON override is never sent to the network —
//!   the request proceeds with the built-in body and a warning is logged.
//! * `model` and `messages` can never be replaced by an override, because
//!   breaking those turns a recoverable typo into a hard request failure.

use serde_json::{Map, Value};

/// Keys that a user-supplied override may never replace.
pub const PROTECTED_REQUEST_KEYS: &[&str] = &["model", "messages"];

/// Maximum accepted size for a raw request-override document, in characters.
pub const MAX_REQUEST_OVERRIDE_CHARS: usize = 64_000;

/// Maximum accepted size for a user-supplied prompt, in characters.
pub const MAX_PROMPT_OVERRIDE_CHARS: usize = 32_000;

/// Parse a user-editable JSON object document.
///
/// Returns `None` when the document is empty or unusable. Callers must treat
/// `None` as "no override", never as an error that aborts the request.
pub fn parse_request_override(raw: &str) -> Option<Map<String, Value>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() > MAX_REQUEST_OVERRIDE_CHARS {
        tracing::warn!(
            "Advanced request override ignored: exceeds {MAX_REQUEST_OVERRIDE_CHARS} characters"
        );
        return None;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(object)) => Some(object),
        Ok(other) => {
            tracing::warn!(
                "Advanced request override ignored: expected a JSON object, got {}",
                json_shape(&other)
            );
            None
        }
        Err(error) => {
            tracing::warn!("Advanced request override ignored: invalid JSON ({error})");
            None
        }
    }
}

/// Merge a user override into a request body using top-level key semantics.
///
/// * New keys are appended.
/// * Existing keys are replaced, so budgets and provider flags can be tuned.
/// * Protected keys are dropped from the override and reported.
pub fn merge_request_override(
    body: &mut Value,
    override_object: &Map<String, Value>,
) -> Vec<String> {
    let mut dropped: Vec<String> = Vec::new();
    let Some(target) = body.as_object_mut() else {
        return dropped;
    };
    for (key, value) in override_object {
        if PROTECTED_REQUEST_KEYS.contains(&key.as_str()) {
            dropped.push(key.clone());
            continue;
        }
        target.insert(key.clone(), value.clone());
    }
    if !dropped.is_empty() {
        tracing::warn!(
            "Advanced request override ignored protected keys: {}",
            dropped.join(", ")
        );
    }
    dropped
}

/// Validate an override document for the settings UI.
///
/// `Ok(None)` means "empty, nothing to apply". `Err` carries a message that is
/// safe to show directly in the desktop UI.
pub fn validate_request_override(raw: &str) -> Result<Option<Map<String, Value>>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_REQUEST_OVERRIDE_CHARS {
        return Err(format!(
            "Request parameters are too long (max {MAX_REQUEST_OVERRIDE_CHARS} characters)"
        ));
    }
    let parsed: Value = serde_json::from_str(trimmed)
        .map_err(|error| format!("Request parameters must be valid JSON: {error}"))?;
    match parsed {
        Value::Object(object) => Ok(Some(object)),
        other => Err(format!(
            "Request parameters must be a JSON object, got {}",
            json_shape(&other)
        )),
    }
}

/// Validate a prompt override for the settings UI.
pub fn validate_prompt_override(raw: &str) -> Result<(), String> {
    if raw.chars().count() > MAX_PROMPT_OVERRIDE_CHARS {
        return Err(format!(
            "Prompt is too long (max {MAX_PROMPT_OVERRIDE_CHARS} characters)"
        ));
    }
    Ok(())
}

/// Append user text to a prompt, ignoring whitespace-only additions.
pub fn append_prompt(base: String, addition: &str) -> String {
    let addition = addition.trim();
    if addition.is_empty() {
        return base;
    }
    let mut prompt = base;
    prompt.push_str("\n\n[ADVANCED_USER_INSTRUCTIONS]\n");
    prompt.push_str(addition);
    prompt
}

/// Clamp a user-supplied token budget into a usable range.
pub fn clamp_max_tokens(value: u32, fallback: u32) -> u32 {
    match value {
        0 => fallback,
        v => v.clamp(16, 128_000),
    }
}

/// Clamp a user-supplied temperature into the OpenAI-compatible range.
pub fn clamp_temperature(value: f64, fallback: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 2.0)
    } else {
        fallback
    }
}

/// Clamp a user-supplied request timeout.
pub fn clamp_timeout_secs(value: u64, fallback: u64) -> u64 {
    match value {
        0 => fallback,
        v => v.clamp(5, 600),
    }
}

fn json_shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_and_malformed_overrides_never_reach_the_network() {
        assert!(parse_request_override("").is_none());
        assert!(parse_request_override("   ").is_none());
        assert!(parse_request_override("{ not json").is_none());
        assert!(parse_request_override(r#"["max_tokens", 10]"#).is_none());
        assert!(parse_request_override("42").is_none());
    }

    #[test]
    fn valid_object_override_is_parsed() {
        let object = parse_request_override(r#"{"reasoning":{"effort":"none"},"top_p":0.9}"#)
            .expect("object");
        assert_eq!(object["top_p"], json!(0.9));
        assert_eq!(object["reasoning"]["effort"], json!("none"));
    }

    #[test]
    fn merge_appends_new_keys_and_replaces_existing_ones() {
        let mut body = json!({"model": "m", "messages": [], "max_tokens": 80, "temperature": 0.2});
        let override_object =
            parse_request_override(r#"{"max_tokens": 4096, "service_tier": "priority"}"#).unwrap();

        merge_request_override(&mut body, &override_object);

        assert_eq!(body["max_tokens"], json!(4096));
        assert_eq!(body["service_tier"], json!("priority"));
        assert_eq!(body["temperature"], json!(0.2));
    }

    #[test]
    fn merge_never_replaces_model_or_messages() {
        let mut body = json!({"model": "kept", "messages": [{"role": "user", "content": "hi"}]});
        let override_object =
            parse_request_override(r#"{"model": "hijacked", "messages": [], "stream": true}"#)
                .unwrap();

        let dropped = merge_request_override(&mut body, &override_object);

        assert_eq!(body["model"], json!("kept"));
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["stream"], json!(true));
        // `serde_json::Map` is a BTreeMap, so report the dropped keys sorted.
        let mut dropped_sorted = dropped.clone();
        dropped_sorted.sort();
        assert_eq!(
            dropped_sorted,
            vec!["messages".to_string(), "model".to_string()]
        );
    }

    #[test]
    fn override_validation_reports_actionable_errors() {
        assert!(validate_request_override("").unwrap().is_none());
        assert!(validate_request_override(r#"{"a":1}"#).unwrap().is_some());
        assert!(validate_request_override("{bad")
            .unwrap_err()
            .contains("valid JSON"));
        assert!(validate_request_override("[1,2]")
            .unwrap_err()
            .contains("JSON object"));
    }

    #[test]
    fn prompt_append_is_ignored_when_blank_and_labeled_when_present() {
        assert_eq!(append_prompt("base".to_string(), "   "), "base");

        let appended = append_prompt("base".to_string(), "  talk like a pirate \n");
        assert!(appended.starts_with("base\n\n[ADVANCED_USER_INSTRUCTIONS]\n"));
        assert!(appended.ends_with("talk like a pirate"));
    }

    #[test]
    fn budgets_and_timeouts_are_clamped_into_usable_ranges() {
        assert_eq!(clamp_max_tokens(0, 4096), 4096);
        assert_eq!(clamp_max_tokens(1, 4096), 16);
        assert_eq!(clamp_max_tokens(999_999, 4096), 128_000);
        assert_eq!(clamp_max_tokens(4096, 4096), 4096);

        assert_eq!(clamp_temperature(f64::NAN, 0.2), 0.2);
        assert_eq!(clamp_temperature(9.0, 0.2), 2.0);
        assert_eq!(clamp_temperature(-1.0, 0.2), 0.0);
        assert_eq!(clamp_temperature(0.7, 0.2), 0.7);

        assert_eq!(clamp_timeout_secs(0, 120), 120);
        assert_eq!(clamp_timeout_secs(1, 120), 5);
        assert_eq!(clamp_timeout_secs(5000, 120), 600);
    }
}
