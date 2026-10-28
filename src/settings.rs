//! Merging settings fragments into `.claude/settings.local.json` entry by entry.
//!
//! A fragment is flattened into items: each list element is one item (added to
//! the list if absent), each scalar is one item (set only if the key is absent).
//! sor records the items it added, so it can remove exactly those later.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub path: Vec<String>,
    pub value: Value,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub set: bool,
}

impl Item {
    pub fn label(&self) -> String {
        self.path.join(".")
    }
}

pub fn flatten(fragment: &Map<String, Value>) -> Vec<Item> {
    let mut out = Vec::new();
    flatten_into(fragment, &mut Vec::new(), &mut out);
    out
}

fn flatten_into(map: &Map<String, Value>, prefix: &mut Vec<String>, out: &mut Vec<Item>) {
    for (k, v) in map {
        prefix.push(k.clone());
        match v {
            Value::Object(m) => flatten_into(m, prefix, out),
            Value::Array(list) => out.extend(list.iter().map(|e| Item {
                path: prefix.clone(),
                value: e.clone(),
                set: false,
            })),
            _ => out.push(Item {
                path: prefix.clone(),
                value: v.clone(),
                set: true,
            }),
        }
        prefix.pop();
    }
}

fn get<'a>(doc: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(doc, |node, k| node.get(k))
}

pub fn present(doc: &Value, item: &Item) -> bool {
    match get(doc, &item.path) {
        Some(v) if item.set => *v == item.value,
        Some(Value::Array(list)) => list.contains(&item.value),
        _ => false,
    }
}

/// A scalar the user already set to another value: sor does not overwrite it.
pub fn clashes(doc: &Value, item: &Item) -> bool {
    item.set && get(doc, &item.path).is_some_and(|v| *v != item.value)
}

/// Add `item`; false when the document's shape is in the way.
pub fn add(doc: &mut Value, item: &Item) -> bool {
    let Some((last, parents)) = item.path.split_last() else {
        return false;
    };
    let mut node = doc;
    for k in parents {
        let Some(map) = node.as_object_mut() else {
            return false;
        };
        node = map
            .entry(k.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let Some(map) = node.as_object_mut() else {
        return false;
    };
    if item.set {
        if map.get(last).is_some_and(|v| *v != item.value) {
            return false;
        }
        map.insert(last.clone(), item.value.clone());
        return true;
    }
    match map
        .entry(last.clone())
        .or_insert_with(|| Value::Array(Vec::new()))
    {
        Value::Array(list) => {
            if !list.contains(&item.value) {
                list.push(item.value.clone());
            }
            true
        }
        _ => false,
    }
}

/// Remove `item` if present, then drop the containers it left empty.
pub fn remove(doc: &mut Value, item: &Item) {
    remove_at(doc, &item.path, item);
}

fn remove_at(node: &mut Value, path: &[String], item: &Item) {
    let Some(map) = node.as_object_mut() else {
        return;
    };
    let Some((key, rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        match map.get_mut(key) {
            Some(v) if item.set && *v == item.value => {
                map.remove(key);
            }
            Some(Value::Array(list)) if !item.set => {
                list.retain(|e| *e != item.value);
                if list.is_empty() {
                    map.remove(key);
                }
            }
            _ => {}
        }
        return;
    }
    if let Some(child) = map.get_mut(key) {
        remove_at(child, rest, item);
        if child.as_object().is_some_and(Map::is_empty) {
            map.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn items(v: Value) -> Vec<Item> {
        flatten(v.as_object().unwrap())
    }

    #[test]
    fn lists_merge_and_user_entries_survive_removal() {
        let mut doc = json!({ "permissions": { "allow": ["Bash(ls)"] } });
        let frag = items(json!({ "permissions": { "allow": ["Bash(ls)", "Bash(git status *)"] } }));
        assert!(present(&doc, &frag[0]));
        for i in &frag {
            assert!(add(&mut doc, i));
        }
        assert_eq!(doc["permissions"]["allow"].as_array().unwrap().len(), 2);
        remove(&mut doc, &frag[1]);
        assert_eq!(doc, json!({ "permissions": { "allow": ["Bash(ls)"] } }));
        remove(&mut doc, &frag[0]);
        assert_eq!(doc, json!({}));
    }

    #[test]
    fn scalars_never_overwrite() {
        let mut doc = json!({ "model": "a" });
        let frag = items(json!({ "model": "b" }));
        assert!(clashes(&doc, &frag[0]));
        assert!(!add(&mut doc, &frag[0]));
        assert_eq!(doc["model"], "a");
    }
}
