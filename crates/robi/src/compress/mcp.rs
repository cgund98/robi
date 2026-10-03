//! JSON and line compression for an MCP tool result.
//!
//! The stored original is the bounded string the tool returned. This module
//! only renders the smaller view.

use serde_json::{json, Map, Value};

use super::shell::compress_stream;

const SMALL: usize = 4 * 1024;
const MIN_SAVINGS: usize = 1024;
const SAMPLE: usize = 3;
const MAX_KEYS: usize = 64;
const MAX_NEST_KEYS: usize = 32;
const STRING_HEAD: usize = 200;
const REST_IDS: usize = 40;
const GENERIC_KEEP: usize = 8;
const OBJECT_COPY: usize = 2 * 1024;
const SAMPLE_LIMIT: usize = 4 * 1024;

/// The smaller body, without the `<<<ROBI_LOG` header.
///
/// `None` means the string stays byte-identical: it is under 4 KiB, it is
/// not a shape this pass folds, or the fold saves under 1 KiB once the
/// header is counted.
pub fn compress_text(input: &str) -> Option<String> {
    if input.len() < SMALL {
        return None;
    }
    match serde_json::from_str::<Value>(input.trim()) {
        Ok(Value::Array(items)) => {
            let body = serde_json::to_string(&render_array(&items)).ok()?;
            saves(&body, input).then_some(body)
        }
        Ok(Value::Object(map)) => {
            let body = serde_json::to_string(&render_object(&map)).ok()?;
            saves(&body, input).then_some(body)
        }
        _ => compress_stream(input),
    }
}

/// True when the client's 256 KiB bound appended its truncation line.
pub fn client_truncated(text: &str) -> bool {
    let Some(line) = text.lines().next_back() else {
        return false;
    };
    let Some(rest) = line.strip_prefix("[truncated at ") else {
        return false;
    };
    let Some(count) = rest.strip_suffix(" bytes]") else {
        return false;
    };
    !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn header(id: &str, sha256: &str) -> String {
    let prefix = &sha256[..sha256.len().min(16)];
    format!(r#"<<<ROBI_LOG id="{id}" sha256="{prefix}" kind=mcp>>>"#)
}

fn saves(body: &str, input: &str) -> bool {
    let header = header(&"0".repeat(36), &"a".repeat(64));
    input.len().saturating_sub(header.len() + 1 + body.len()) >= MIN_SAVINGS
}

fn render_array(items: &[Value]) -> Value {
    if items.iter().all(Value::is_object) {
        render_object_array(items)
    } else {
        render_generic_array(items)
    }
}

fn render_object_array(items: &[Value]) -> Value {
    let mut keys = Vec::new();
    let mut types: Map<String, Value> = Map::new();
    let mut extra = 0usize;
    let mut seen_extra = Vec::new();
    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        for (key, value) in object {
            if keys.iter().any(|kept: &String| kept == key) {
                push_type(&mut types, key, value);
                continue;
            }
            if keys.len() == MAX_KEYS {
                if !seen_extra.iter().any(|kept: &String| kept == key) {
                    seen_extra.push(key.clone());
                    extra += 1;
                }
                continue;
            }
            keys.push(key.clone());
            push_type(&mut types, key, value);
        }
    }
    let mut sample = Vec::new();
    let mut sample_omitted = 0usize;
    for item in items.iter().take(SAMPLE) {
        match fit_sample(item) {
            Some(value) => sample.push(value),
            None => sample_omitted += 1,
        }
    }
    let mut body = json!({
        "compressed": "mcp-json",
        "items": items.len(),
        "keys": keys,
        "types": types,
        "sample": sample,
    });
    if extra > 0 {
        body["keys_omitted"] = json!(extra);
    }
    if sample_omitted > 0 {
        body["sample_omitted"] = json!(sample_omitted);
    }
    if let Some(key) = id_key(items) {
        let mut rest_ids = Vec::new();
        for item in items.iter().skip(SAMPLE) {
            if rest_ids.len() == REST_IDS {
                break;
            }
            if let Some(value) = item.get(&key) {
                if value.is_string() || value.is_number() {
                    rest_ids.push(value.clone());
                }
            }
        }
        body["rest_ids"] = json!(rest_ids);
        body["rest_omitted"] = json!(items.len().saturating_sub(sample.len() + rest_ids.len()));
    } else {
        body["rest_omitted"] = json!(items.len().saturating_sub(sample.len()));
    }
    body
}

fn render_generic_array(items: &[Value]) -> Value {
    let mut sample = Vec::new();
    for item in items.iter().take(GENERIC_KEEP) {
        if let Some(value) = fit_sample(item) {
            sample.push(value);
        }
    }
    json!({
        "compressed": "mcp-json",
        "items": items.len(),
        "sample": sample,
        "omitted": items.len().saturating_sub(sample.len()),
    })
}

fn render_object(map: &Map<String, Value>) -> Value {
    let mut body = Map::new();
    body.insert("compressed".into(), json!("mcp-json"));
    for (key, value) in map {
        body.insert(key.clone(), fold_field(value));
    }
    Value::Object(body)
}

fn fold_field(value: &Value) -> Value {
    match value {
        Value::String(text) if text.chars().count() > STRING_HEAD => json!({
            "_chars": text.chars().count(),
            "_head": char_prefix(text, STRING_HEAD),
        }),
        Value::Array(items) => render_array(items),
        Value::Object(map) => {
            let raw = serde_json::to_string(map).unwrap_or_default();
            if raw.len() <= OBJECT_COPY {
                value.clone()
            } else {
                let keys: Vec<&String> = map.keys().take(MAX_NEST_KEYS).collect();
                json!({ "_keys": keys })
            }
        }
        other => other.clone(),
    }
}

fn fit_sample(value: &Value) -> Option<Value> {
    let raw = serde_json::to_string(value).ok()?;
    if raw.len() <= SAMPLE_LIMIT {
        return Some(value.clone());
    }
    let shrunk = shrink_element(value);
    let again = serde_json::to_string(&shrunk).ok()?;
    if again.len() > SAMPLE_LIMIT {
        None
    } else {
        Some(shrunk)
    }
}

fn shrink_element(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return shrink_loose(value);
    };
    let mut out = Map::new();
    for (key, field) in object {
        out.insert(key.clone(), shrink_field(field));
    }
    Value::Object(out)
}

fn shrink_loose(value: &Value) -> Value {
    match value {
        Value::Array(items) => json!({ "_items": items.len() }),
        Value::String(text) if text.chars().count() > STRING_HEAD => {
            json!(format!("{}…", char_prefix(text, STRING_HEAD)))
        }
        other => other.clone(),
    }
}

fn shrink_field(value: &Value) -> Value {
    match value {
        Value::Array(items) => json!({ "_items": items.len() }),
        Value::Object(map) => {
            let keys: Vec<&String> = map.keys().take(MAX_NEST_KEYS).collect();
            json!({ "_keys": keys })
        }
        Value::String(text) if text.chars().count() > STRING_HEAD => {
            json!(format!("{}…", char_prefix(text, STRING_HEAD)))
        }
        other => other.clone(),
    }
}

fn id_key(items: &[Value]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    for key in ["id", "identifier", "name"] {
        let hits = items
            .iter()
            .filter(|item| {
                item.get(key)
                    .is_some_and(|value| value.is_string() || value.is_number())
            })
            .count();
        if hits.saturating_mul(100) >= items.len().saturating_mul(80) {
            return Some(key.to_owned());
        }
    }
    None
}

fn push_type(types: &mut Map<String, Value>, key: &str, value: &Value) {
    let name = json_type(value);
    let entry = types.entry(key.to_owned()).or_insert_with(|| json!([]));
    let Some(list) = entry.as_array_mut() else {
        return;
    };
    if !list.iter().any(|item| item.as_str() == Some(name)) {
        list.push(json!(name));
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
    }
}

fn char_prefix(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_array_of_objects_keeps_a_sample_and_the_ids() {
        let mut items = Vec::new();
        for index in 0..200 {
            items.push(format!(
                r#"{{"id":"issue-{index}","title":"Pay invoice {index}","state":"open"}}"#
            ));
        }
        let input = format!("[{}]", items.join(","));
        let body = compress_text(&input).expect("folded");
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["items"], 200);
        assert_eq!(value["sample"].as_array().unwrap().len(), 3);
        assert_eq!(value["rest_ids"].as_array().unwrap().len(), 40);
        assert_eq!(value["rest_omitted"], 200 - 3 - 40);
        assert_eq!(value["keys"][0], "id");
    }

    #[test]
    fn a_huge_nested_object_in_a_sample_becomes_keys() {
        let nested = "x".repeat(5000);
        let mut items = Vec::new();
        for index in 0..30 {
            items.push(format!(
                r#"{{"id":"{index}","meta":{{"blob":"{nested}"}}}}"#
            ));
        }
        let input = format!("[{}]", items.join(","));
        let body = compress_text(&input).expect("folded");
        let value: Value = serde_json::from_str(&body).unwrap();
        let meta = &value["sample"][0]["meta"];
        assert!(meta["_keys"].is_array(), "{meta}");
    }

    #[test]
    fn a_non_object_array_keeps_eight_elements() {
        let notes: Vec<String> = (0..40)
            .map(|index| format!("\"{}\"", "n".repeat(200) + &index.to_string()))
            .collect();
        let input = format!("[{}]", notes.join(","));
        assert!(input.len() > SMALL);
        let body = compress_text(&input).expect("folded");
        let value: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["sample"].as_array().unwrap().len(), 8);
        assert_eq!(value["omitted"], 32);
        assert!(value.get("keys").is_none());
        assert!(value.get("rest_ids").is_none());
    }

    #[test]
    fn a_brace_prefix_that_does_not_parse_is_not_the_json_pass() {
        let mut input = "{\n".to_owned();
        for index in 0..200 {
            input.push_str(&format!("test tests::test_auth_{index} ... ok\n"));
        }
        let body = compress_text(&input).expect("line pass");
        assert!(!body.contains("\"compressed\""));
        assert!(body.contains("ROBI_LOG") || body.contains("... ok"));
    }

    #[test]
    fn a_body_that_saves_under_one_kib_returns_none() {
        let input = format!("[{}]", r#"{"id":"only","title":"short"}"#.repeat(1));
        let padded = format!("{input}{}", " ".repeat(SMALL));
        // Leading spaces then one small object array still parses after trim,
        // and the rendered form is not 1 KiB smaller than the padded original
        // when the padding is only the gate. Use a JSON array that is just
        // over 4 KiB of distinct prose so the fold cannot collapse it.
        let sentence = "The payment service validates the expiry and nothing repeats. ";
        let mut text = String::from("{");
        let mut index = 0;
        while text.len() < SMALL + 100 {
            text.push_str(&format!("\"k{index}\":\"{sentence}{index}\","));
            index += 1;
        }
        text.pop();
        text.push('}');
        assert!(compress_text(&text).is_none(), "dense object");
        let _ = padded;
    }

    #[test]
    fn truncation_line_is_detected() {
        assert!(client_truncated("hello\n[truncated at 262144 bytes]"));
        assert!(!client_truncated("hello\n[truncated at bytes]"));
    }
}
