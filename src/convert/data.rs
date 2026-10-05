//! Structured data: JSON, YAML, TOML, XML and their bridges to tables.

use super::table::{as_number, Sheet};
use crate::formats::Format;
use crate::textenc::read_text_file;
use anyhow::{anyhow, bail, Context, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde_json::{Map, Value};
use std::path::Path;

pub fn read_value(path: &Path, format: Format) -> Result<Value> {
    let text = read_text_file(path)?;
    parse_value(&text, format)
}

pub fn parse_value(text: &str, format: Format) -> Result<Value> {
    Ok(match format {
        Format::Json => serde_json::from_str(text).context("invalid JSON")?,
        Format::Yaml => serde_yaml::from_str(text).context("invalid YAML")?,
        Format::Toml => toml::from_str(text).context("invalid TOML")?,
        Format::Xml => xml_to_value(text)?,
        _ => bail!("{} is not a data format", format.label()),
    })
}

pub fn render_value(value: &Value, format: Format) -> Result<String> {
    Ok(match format {
        Format::Json => serde_json::to_string_pretty(value)? + "\n",
        Format::Yaml => serde_yaml::to_string(value)?,
        Format::Toml => {
            let mut v = strip_nulls(value.clone());
            if !v.is_object() {
                let mut m = Map::new();
                m.insert("items".to_string(), v);
                v = Value::Object(m);
            }
            toml::to_string_pretty(&v).map_err(|e| anyhow!("cannot express this data as TOML: {e}"))?
        }
        Format::Xml => value_to_xml(value),
        _ => bail!("{} is not a data format", format.label()),
    })
}

/// TOML has no null.
fn strip_nulls(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.into_iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k, strip_nulls(v))).collect()),
        Value::Array(a) => Value::Array(a.into_iter().filter(|v| !v.is_null()).map(strip_nulls).collect()),
        other => other,
    }
}

// ---------------------------------------------------------------- XML

fn insert_child(map: &mut Map<String, Value>, key: String, value: Value) {
    match map.get_mut(&key) {
        // Arrays under a key only ever come from repeated siblings.
        Some(Value::Array(arr)) => arr.push(value),
        Some(existing) => {
            let prev = existing.take();
            *existing = Value::Array(vec![prev, value]);
        }
        None => {
            map.insert(key, value);
        }
    }
}

struct Frame {
    name: String,
    map: Map<String, Value>,
    text: String,
}

fn finish_frame(frame: Frame) -> Value {
    let text = frame.text.trim().to_string();
    if frame.map.is_empty() {
        if text.is_empty() {
            Value::Null
        } else {
            Value::String(text)
        }
    } else {
        let mut map = frame.map;
        if !text.is_empty() {
            map.insert("#text".to_string(), Value::String(text));
        }
        Value::Object(map)
    }
}

pub fn xml_to_value(xml: &str) -> Result<Value> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Frame> = vec![Frame { name: String::new(), map: Map::new(), text: String::new() }];

    let start_frame = |e: &quick_xml::events::BytesStart| -> Frame {
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        let mut map = Map::new();
        for a in e.attributes().flatten() {
            let key = format!("@{}", String::from_utf8_lossy(a.key.as_ref()));
            let val = a.unescape_value().map(|v| v.into_owned()).unwrap_or_default();
            map.insert(key, Value::String(val));
        }
        Frame { name, map, text: String::new() }
    };

    loop {
        match reader.read_event().context("invalid XML")? {
            Event::Start(e) => stack.push(start_frame(&e)),
            Event::Empty(e) => {
                let frame = start_frame(&e);
                let name = frame.name.clone();
                let value = finish_frame(frame);
                if let Some(parent) = stack.last_mut() {
                    insert_child(&mut parent.map, name, value);
                }
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t.unescape().map(|c| c.into_owned()).unwrap_or_default());
                }
            }
            Event::CData(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    bail!("invalid XML: unexpected closing tag");
                }
                let frame = stack.pop().expect("stack has two frames");
                let name = frame.name.clone();
                let value = finish_frame(frame);
                if let Some(parent) = stack.last_mut() {
                    insert_child(&mut parent.map, name, value);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let root = stack.into_iter().next().map(|f| f.map).unwrap_or_default();
    Ok(Value::Object(root))
}

fn xml_name(key: &str) -> String {
    let mut out: String = key
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':' { c } else { '_' })
        .collect();
    if out.is_empty() || !out.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false) {
        out.insert(0, '_');
    }
    out
}

fn xml_text(s: &str) -> String {
    crate::doc::write_docx::xml_escape(s)
}

fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn write_element(out: &mut String, name: &str, value: &Value, depth: usize) {
    let indent = "  ".repeat(depth);
    let tag = xml_name(name);
    match value {
        Value::Array(items) => {
            for item in items {
                write_element(out, name, item, depth);
            }
        }
        Value::Object(map) => {
            out.push_str(&indent);
            out.push('<');
            out.push_str(&tag);
            for (k, v) in map {
                if let Some(attr) = k.strip_prefix('@') {
                    out.push_str(&format!(" {}=\"{}\"", xml_name(attr), xml_text(&scalar_text(v))));
                }
            }
            let children: Vec<(&String, &Value)> = map.iter().filter(|(k, _)| !k.starts_with('@')).collect();
            if children.is_empty() {
                out.push_str("/>\n");
                return;
            }
            if children.len() == 1 && children[0].0 == "#text" {
                out.push_str(&format!(">{}</{tag}>\n", xml_text(&scalar_text(children[0].1))));
                return;
            }
            out.push_str(">\n");
            for (k, v) in children {
                if k == "#text" {
                    out.push_str(&format!("{indent}  {}\n", xml_text(&scalar_text(v))));
                } else {
                    write_element(out, k, v, depth + 1);
                }
            }
            out.push_str(&format!("{indent}</{tag}>\n"));
        }
        Value::Null => out.push_str(&format!("{indent}<{tag}/>\n")),
        other => out.push_str(&format!("{indent}<{tag}>{}</{tag}>\n", xml_text(&scalar_text(other)))),
    }
}

pub fn value_to_xml(value: &Value) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    match value {
        Value::Object(map) if map.len() == 1 && !map.values().next().map(|v| v.is_array()).unwrap_or(false) => {
            let (k, v) = map.iter().next().expect("one entry");
            write_element(&mut out, k, v, 0);
        }
        Value::Array(items) => {
            out.push_str("<root>\n");
            for item in items {
                write_element(&mut out, "item", item, 1);
            }
            out.push_str("</root>\n");
        }
        other => write_element(&mut out, "root", other, 0),
    }
    out
}

// ---------------------------------------------------------------- tables

fn cell(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Object(_) | Value::Array(_) => serde_json::to_string(v).unwrap_or_default(),
        other => other.to_string(),
    }
}

fn array_to_rows(items: &[Value]) -> Vec<Vec<String>> {
    if !items.is_empty() && items.iter().all(|v| v.is_object()) {
        let mut headers: Vec<String> = Vec::new();
        for item in items {
            if let Value::Object(m) = item {
                for k in m.keys() {
                    if !headers.contains(k) {
                        headers.push(k.clone());
                    }
                }
            }
        }
        let mut rows = vec![headers.clone()];
        for item in items {
            if let Value::Object(m) = item {
                rows.push(headers.iter().map(|h| m.get(h).map(cell).unwrap_or_default()).collect());
            }
        }
        rows
    } else if items.iter().all(|v| v.is_array()) {
        items
            .iter()
            .map(|v| v.as_array().map(|a| a.iter().map(cell).collect()).unwrap_or_default())
            .collect()
    } else {
        let mut rows = vec![vec!["value".to_string()]];
        rows.extend(items.iter().map(|v| vec![cell(v)]));
        rows
    }
}

pub fn value_to_sheets(value: &Value, default_name: &str) -> Result<Vec<Sheet>> {
    match value {
        Value::Array(items) => Ok(vec![Sheet { name: default_name.to_string(), rows: array_to_rows(items) }]),
        Value::Object(map) => {
            // A single wrapper key around an array, e.g. {"users": [...]}.
            if map.len() == 1 {
                if let Some((k, Value::Array(items))) = map.iter().next() {
                    return Ok(vec![Sheet { name: k.clone(), rows: array_to_rows(items) }]);
                }
            }
            if !map.is_empty() && map.values().all(|v| v.is_array()) {
                return Ok(map
                    .iter()
                    .map(|(k, v)| Sheet { name: k.clone(), rows: array_to_rows(v.as_array().map(|a| a.as_slice()).unwrap_or(&[])) })
                    .collect());
            }
            let mut rows = vec![vec!["key".to_string(), "value".to_string()]];
            for (k, v) in map {
                rows.push(vec![k.clone(), cell(v)]);
            }
            Ok(vec![Sheet { name: default_name.to_string(), rows }])
        }
        other => Ok(vec![Sheet { name: default_name.to_string(), rows: vec![vec![cell(other)]] }]),
    }
}

fn typed(s: &str) -> Value {
    if let Some(n) = as_number(s) {
        if n.fract() == 0.0 && n.abs() < 9e15 && !s.contains('.') && !s.contains('e') && !s.contains('E') {
            return Value::from(n as i64);
        }
        if let Some(num) = serde_json::Number::from_f64(n) {
            return Value::Number(num);
        }
    }
    match s {
        "true" | "TRUE" | "True" => Value::Bool(true),
        "false" | "FALSE" | "False" => Value::Bool(false),
        _ => Value::String(s.to_string()),
    }
}

fn sheet_to_value(sheet: &Sheet) -> Value {
    let Some((head, body)) = sheet.rows.split_first() else {
        return Value::Array(Vec::new());
    };
    let mut headers: Vec<String> = Vec::new();
    for (i, h) in head.iter().enumerate() {
        let mut name = h.trim().to_string();
        if name.is_empty() {
            name = format!("column_{}", i + 1);
        }
        let base = name.clone();
        let mut n = 2;
        while headers.contains(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        headers.push(name);
    }
    let width = sheet.rows.iter().map(|r| r.len()).max().unwrap_or(0);
    while headers.len() < width {
        headers.push(format!("column_{}", headers.len() + 1));
    }
    let items = body
        .iter()
        .map(|row| {
            let mut m = Map::new();
            for (i, h) in headers.iter().enumerate() {
                let v = row.get(i).map(|s| typed(s)).unwrap_or(Value::Null);
                m.insert(h.clone(), v);
            }
            Value::Object(m)
        })
        .collect();
    Value::Array(items)
}

pub fn sheets_to_value(sheets: &[Sheet]) -> Value {
    if sheets.len() == 1 {
        return sheet_to_value(&sheets[0]);
    }
    let mut m = Map::new();
    for s in sheets {
        m.insert(s.name.clone(), sheet_to_value(s));
    }
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_yaml_toml_roundtrip() {
        let v: Value = serde_json::from_str(r#"{"name":"x","n":3,"tags":["a","b"],"nested":{"ok":true}}"#).unwrap();
        for f in [Format::Yaml, Format::Toml, Format::Json] {
            let text = render_value(&v, f).unwrap();
            let back = parse_value(&text, f).unwrap();
            assert_eq!(back, v, "roundtrip through {f:?}");
        }
    }

    #[test]
    fn xml_roundtrip_shape() {
        let v = xml_to_value("<people><person id=\"1\"><name>A</name></person><person id=\"2\"><name>B</name></person></people>").unwrap();
        let persons = &v["people"]["person"];
        assert!(persons.is_array());
        assert_eq!(persons[1]["@id"], "2");
        let xml = value_to_xml(&v);
        assert!(xml.contains("<person id=\"2\">"));
    }

    #[test]
    fn tables_bridge() {
        let v: Value = serde_json::from_str(r#"[{"a":1,"b":"x"},{"a":2,"c":"007"}]"#).unwrap();
        let sheets = value_to_sheets(&v, "data").unwrap();
        assert_eq!(sheets[0].rows[0], vec!["a", "b", "c"]);
        let back = sheets_to_value(&sheets);
        assert_eq!(back[1]["c"], "007");
        assert_eq!(back[0]["a"], 1);
    }
}
