//! Tags (`tags/<registry>/*.json`), resolved to the ids they hold: block tags here, and any
//! other registry's for the crates above.

use std::collections::HashSet;

use serde_json::Value;
use spyglass_data::bundle::BundleError;
use spyglass_data::codec::{At, LoadError, LoadErrorKind, full_id};
use spyglass_density::Resources;

/// The registry path of block tags.
const BLOCK_TAG: &str = "tags/block";

/// The block ids in tag `id`, as [`tag`] resolves them.
pub(crate) fn block_tag<R: Resources + ?Sized>(
    resources: &R,
    id: &str,
) -> Result<HashSet<String>, LoadError> {
    Ok(tag(resources, BLOCK_TAG, id)?.into_iter().collect())
}

/// The ids in tag `id` of `registry` (a tag registry path, e.g. `tags/block`), nested tags
/// included (`TagEntry.CODEC`: an id, a `#tag`, or `{"id": ..., "required": bool}`), each once,
/// in the order the entries first name them. There's no registry in a bundle to check ids
/// against, so every id is kept; only a missing optional tag is skipped.
pub fn tag<R: Resources + ?Sized>(
    resources: &R,
    registry: &str,
    id: &str,
) -> Result<Vec<String>, LoadError> {
    let mut values = Vec::new();
    resolve(
        resources,
        registry,
        &full_id(id),
        &mut Vec::new(),
        &mut values,
    )?;
    Ok(values)
}

fn resolve<R: Resources + ?Sized>(
    resources: &R,
    registry: &str,
    id: &str,
    loading: &mut Vec<String>,
    blocks: &mut Vec<String>,
) -> Result<(), LoadError> {
    let resource = format!("#{id}");
    let root = At::root(&resource);
    if loading.iter().any(|t| t == id) {
        let mut cycle = loading.clone();
        cycle.push(id.to_string());
        return Err(root.error(LoadErrorKind::Cycle(cycle)));
    }
    let json = resources.resource(registry, id).map_err(|e| match e {
        BundleError::NotFound(_) if loading.is_empty() => root.error(LoadErrorKind::Invalid(
            "the bundle has no such tag; extract it again with this version of spyglass",
        )),
        e => root.error(e.into()),
    })?;
    let values = json
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| root.error(LoadErrorKind::Missing("values")))?;
    loading.push(id.to_string());
    for (i, entry) in values.iter().enumerate() {
        let at = root.child(&format!("values[{i}]"));
        let (entry_id, required) = match entry {
            Value::String(s) => (s.as_str(), true),
            Value::Object(obj) => (
                obj.get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| at.error(LoadErrorKind::Missing("id")))?,
                match obj.get("required") {
                    None => true,
                    Some(v) => v.as_bool().ok_or_else(|| {
                        at.child("required")
                            .error(LoadErrorKind::Shape("a boolean"))
                    })?,
                },
            ),
            _ => return Err(at.error(LoadErrorKind::Shape("a tag entry"))),
        };
        match entry_id.strip_prefix('#') {
            Some(tag) => {
                let tag = full_id(tag);
                if !required && resources.resource(registry, &tag).is_err() {
                    continue;
                }
                resolve(resources, registry, &tag, loading, blocks)?;
            }
            None => {
                let id = full_id(entry_id);
                if !blocks.contains(&id) {
                    blocks.push(id);
                }
            }
        }
    }
    loading.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;

    fn tags(entries: &[(&str, Value)]) -> HashMap<String, Value> {
        entries
            .iter()
            .map(|(id, v)| (format!("tags/block/minecraft:{id}"), v.clone()))
            .collect()
    }

    #[test]
    fn nested_and_optional_entries() {
        let map = tags(&[
            (
                "a",
                json!({"values": ["stone", "#b", {"id": "#gone", "required": false}]}),
            ),
            (
                "b",
                json!({"values": ["minecraft:dirt", {"id": "mod:x", "required": false}]}),
            ),
        ]);
        let got = block_tag(&map, "a").unwrap();
        let want: HashSet<String> = ["minecraft:stone", "minecraft:dirt", "mod:x"]
            .map(String::from)
            .into();
        assert_eq!(got, want);
    }

    #[test]
    fn cycles_and_missing_tags_are_errors() {
        let map = tags(&[
            ("a", json!({"values": ["#b"]})),
            ("b", json!({"values": ["#a"]})),
            ("c", json!({"values": ["#gone"]})),
        ]);
        assert!(matches!(
            *block_tag(&map, "a").unwrap_err().kind,
            LoadErrorKind::Cycle(_)
        ));
        assert!(block_tag(&map, "c").is_err());
        let err = block_tag(&map, "missing").unwrap_err();
        assert!(err.to_string().contains("extract it again"), "{err}");
    }
}
