use serde_json::{Map, Value};

pub fn canonical_json(value: &Value) -> Result<String, String> {
    write(value)
}

fn write(value: &Value) -> Result<String, String> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(true) => Ok("true".to_string()),
        Value::Bool(false) => Ok("false".to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => serde_json::to_string(s).map_err(|e| e.to_string()),
        Value::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&write(item)?);
            }
            out.push(']');
            Ok(out)
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = String::from("{");
            let mut first = true;
            for key in keys {
                if let Some(val) = map.get(key) {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    out.push_str(&serde_json::to_string(key).map_err(|e| e.to_string())?);
                    out.push(':');
                    out.push_str(&write(val)?);
                }
            }
            out.push('}');
            Ok(out)
        }
    }
}

pub fn unsigned_value(bundle: &Value) -> Result<Value, String> {
    let obj = bundle
        .as_object()
        .ok_or_else(|| "bundle must be a JSON object".to_string())?;
    let mut next = Map::new();
    for (k, v) in obj {
        if k != "signature" {
            next.insert(k.clone(), v.clone());
        }
    }
    next.insert("signature".into(), Value::Null);
    Ok(Value::Object(next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_object_keys() {
        let value = json!({"b": 1, "a": {"d": true, "c": null}});
        assert_eq!(
            canonical_json(&value).unwrap(),
            r#"{"a":{"c":null,"d":true},"b":1}"#
        );
    }
}
