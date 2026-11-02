//! Narrow native configuration patches; raw values never leave this module's caller.
use crate::{AppError, mutation::refused};
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item, Table, value};
fn schema() -> AppError {
    refused(
        "Configuration schema or selector is unsupported or ambiguous; preserve it and use the native agent to edit this setting.",
    )
}

/// Replace a value span or insert one object key, preserving every other input byte.
pub(crate) fn json_set(text: &str, keys: &[&str], new: Value) -> Result<String, AppError> {
    crate::json_config::parse(text).map_err(|_| schema())?;
    fn patch(text: &str, keys: &[&str], new: &Value) -> Result<String, AppError> {
        let map: BTreeMap<String, &RawValue> = serde_json::from_str(text).map_err(|_| schema())?;
        let Some((key, rest)) = keys.split_first() else {
            return serde_json::to_string(new).map_err(|_| schema());
        };
        if let Some(raw) = map.get(*key) {
            let start = raw.get().as_ptr() as usize - text.as_ptr() as usize;
            let replaced = if rest.is_empty() {
                serde_json::to_string(new).map_err(|_| schema())?
            } else {
                patch(raw.get(), rest, new)?
            };
            return Ok(format!(
                "{}{}{}",
                &text[..start],
                replaced,
                &text[start + raw.get().len()..]
            ));
        }
        let mut nested = new.clone();
        for key in rest.iter().rev() {
            nested = serde_json::json!({*key: nested});
        }
        let close = text.rfind('}').ok_or_else(schema)?;
        let key = serde_json::to_string(key).map_err(|_| schema())?;
        Ok(format!(
            "{}{}\n  {}: {}\n{}",
            &text[..close],
            if map.is_empty() { "" } else { "," },
            key,
            nested,
            &text[close..]
        ))
    }
    patch(text, keys, &new)
}
pub(crate) fn toml_mcp(text: &str, name: &str, enabled: bool) -> Result<String, AppError> {
    let mut doc = text.parse::<DocumentMut>().map_err(|_| schema())?;
    let entry = doc
        .get_mut("mcp_servers")
        .and_then(|i| i.get_mut(name))
        .ok_or_else(schema)?;
    let table = entry.as_table_like_mut().ok_or_else(schema)?;
    if table.get("enabled").is_some_and(|v| v.as_bool().is_none()) {
        return Err(schema());
    }
    if table.get("enabled").and_then(Item::as_bool).unwrap_or(true) == enabled {
        return Ok(text.into());
    }
    let mut replacement = value(enabled);
    if let Some(v) = table.get("enabled").and_then(Item::as_value) {
        *replacement.as_value_mut().unwrap().decor_mut() = v.decor().clone();
    }
    table.insert("enabled", replacement);
    finish_toml(text, doc.to_string())
}
pub(crate) fn toml_skill(
    text: &str,
    path: &str,
    name: &str,
    enabled: bool,
) -> Result<String, AppError> {
    let parsed: toml::Value = toml::from_str(text).map_err(|_| schema())?;
    let current = parsed
        .get("skills")
        .and_then(|s| s.get("config"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|rule| {
            rule.get("name")
                .and_then(toml::Value::as_str)
                .map(str::trim)
                == Some(name)
                || rule
                    .get("path")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|p| {
                        let p = std::path::Path::new(p);
                        p.canonicalize().unwrap_or_else(|_| p.to_owned())
                            == std::path::Path::new(path)
                    })
        })
        .filter_map(|rule| rule.get("enabled").and_then(toml::Value::as_bool))
        .next_back();
    if current == Some(enabled) {
        return Ok(text.into());
    }
    let mut doc = text.parse::<DocumentMut>().map_err(|_| schema())?;
    if doc.get("skills").is_none() {
        doc["skills"] = Item::Table(Table::new());
    }
    let skills = doc
        .get_mut("skills")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(schema)?;
    if skills.get("config").is_none() {
        skills.insert("config", Item::ArrayOfTables(Default::default()));
    }
    let config = skills.get_mut("config").unwrap();
    // Both native TOML array shapes are supported, without reserializing unrelated fields.
    if let Some(tables) = config.as_array_of_tables_mut() {
        let last = tables.iter().last();
        if let Some(last) = last
            && last.get("path").and_then(Item::as_str) == Some(path)
        {
            if last.get("enabled").and_then(Item::as_bool) == Some(enabled) {
                return Ok(text.into());
            }
            let last = tables.iter_mut().last().unwrap();
            let mut replacement = value(enabled);
            if let Some(v) = last.get("enabled").and_then(Item::as_value) {
                *replacement.as_value_mut().unwrap().decor_mut() = v.decor().clone();
            }
            last.insert("enabled", replacement);
        } else {
            let mut rule = Table::new();
            rule.insert("path", value(path));
            rule.insert("enabled", value(enabled));
            tables.push(rule);
        }
    } else if let Some(array) = config.as_array_mut() {
        let mut rule = toml_edit::InlineTable::new();
        rule.insert("path", path.into());
        rule.insert("enabled", enabled.into());
        if let Some(last) = array
            .iter_mut()
            .last()
            .and_then(toml_edit::Value::as_inline_table_mut)
            && last.get("path").and_then(toml_edit::Value::as_str) == Some(path)
        {
            if last.get("enabled").and_then(toml_edit::Value::as_bool) == Some(enabled) {
                return Ok(text.into());
            }
            last.insert("enabled", enabled.into());
        } else {
            array.push(rule);
        }
    } else {
        return Err(schema());
    }
    finish_toml(text, doc.to_string())
}

// toml_edit emits LF. Restore a uniform CRLF convention only when reparsing proves
// it does not change values (including literal/multiline strings). Mixed styles refuse.
fn finish_toml(original: &str, edited: String) -> Result<String, AppError> {
    let crlf = original.matches("\r\n").count();
    if crlf == 0 {
        return Ok(edited);
    }
    if crlf != original.bytes().filter(|b| *b == b'\n').count() {
        return Err(refused(
            "Mixed TOML line endings cannot be preserved safely; normalize them explicitly in the native configuration before retrying.",
        ));
    }
    let restored = edited.replace("\r\n", "\n").replace('\n', "\r\n");
    let expected: toml::Value = toml::from_str(&edited).map_err(|_| schema())?;
    let actual: toml::Value = toml::from_str(&restored).map_err(|_| schema())?;
    if actual != expected {
        return Err(refused(
            "TOML line-ending preservation could change literal values; no configuration was written.",
        ));
    }
    Ok(restored)
}
