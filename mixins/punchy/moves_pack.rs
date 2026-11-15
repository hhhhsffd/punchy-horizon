//! External Moves packs. Reads Horizon's resolved resource tree, never downloads
//! assets or loads Java. Files and accumulated decompressed input are bounded.
use crate::Clip;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub struct Pack {
    pub clips: BTreeMap<String, Arc<Clip>>,
    pub config: Value,
    pub files: usize,
    pub warnings: Vec<String>,
}

fn files(root: &Path, out: &mut Vec<std::path::PathBuf>, depth: usize) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    if depth > 16 {
        return Err("Moves pack directory nesting exceeds 16".into());
    }
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            files(&entry.path(), out, depth + 1)?;
        } else if kind.is_file() && entry.path().extension().is_some_and(|x| x == "json") {
            out.push(entry.path());
            if out.len() > 4096 {
                return Err("Too many Moves resource files".into());
            }
        }
    }
    Ok(())
}

/// Merge objects recursively; arrays and scalars are replaced, not concatenated.
pub fn merge(base: &mut Value, overlay: &Value) {
    if let (Some(base), Some(over)) = (base.as_object_mut(), overlay.as_object()) {
        for (key, value) in over {
            merge(base.entry(key).or_insert(Value::Null), value);
        }
    } else {
        *base = overlay.clone();
    }
}

impl Pack {
    pub fn load(resources: &Path) -> Result<Self, String> {
        let root = resources.join("assets/punchy_moves");
        let mut paths = Vec::new();
        files(&root.join("animations"), &mut paths, 0)?;
        files(&root.join("compat"), &mut paths, 0)?;
        paths.sort();
        let mut pack = Self::default();
        let mut total = 0;
        for path in paths {
            let size = path.metadata().map_err(|e| e.to_string())?.len();
            total += size;
            if size > MAX_FILE || total > MAX_TOTAL {
                return Err("Moves resource pack exceeds input size limit".into());
            }
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .map_err(|e| e.to_string())?
                .take(MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_FILE {
                return Err("Moves resource file grew beyond limit".into());
            }
            let parsed = (|| {
                let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if path.starts_with(root.join("animations")) {
                    for (name, clip) in crate::load_model_clips(&bytes)? {
                        pack.clips.insert(name, Arc::new(clip));
                    }
                } else {
                    merge(&mut pack.config, &normalize(&value)?);
                }
                Ok::<(), String>(())
            })();
            match parsed {
                Ok(()) => pack.files += 1,
                Err(e) => pack.warnings.push(format!("{}: {e}", path.display())),
            }
        }
        Ok(pack)
    }

    pub fn resolve(&self, name: &str, left: bool) -> Option<Arc<Clip>> {
        let side = if left { "left" } else { "right" };
        self.clips
            .get(&format!("{name}_{side}"))
            .or_else(|| {
                name.strip_suffix("_alt")
                    .and_then(|name| self.clips.get(&format!("{name}_{side}_alt")))
            })
            .or_else(|| self.clips.get(name))
            .cloned()
    }

    pub fn entry(&self, group: &str, action: &str) -> Value {
        let mut entry = self.config["defaults"]["animations"][action].clone();
        let specific = if group == "EMPTY" {
            &self.config["empty_hand"]["animations"][action]
        } else {
            &self.config["item_groups"][group]["animations"][action]
        };
        if !specific.is_null() {
            if specific.is_string() {
                return serde_json::json!({"clip":specific});
            }
            merge(&mut entry, specific);
        }
        if entry.is_string() {
            entry = serde_json::json!({"clip":entry});
        }
        entry
    }

    pub fn definition(&self, item_id: &str) -> Value {
        let mut definition = self.config["defaults"].clone();
        let group = item_group(item_id);
        let group_definition = if group == "EMPTY" {
            &self.config["empty_hand"]
        } else {
            &self.config["item_groups"][group]
        };
        if !group_definition.is_null() {
            merge(&mut definition, group_definition);
        }
        if let Some(items) = self.config["items"].as_object() {
            for (selector, over) in items {
                if selector == item_id
                    || selector
                        .split_once('*')
                        .is_some_and(|(a, b)| item_id.starts_with(a) && item_id.ends_with(b))
                {
                    merge(&mut definition, over);
                }
            }
        }
        definition
    }
    pub fn entry_for_item(&self, item_id: &str, action: &str) -> Value {
        let definition = self.definition(item_id);
        let mut entry = definition["animations"][action].clone();
        if entry.is_string() {
            entry = serde_json::json!({"clip":entry});
        }
        if definition["sequence"].is_object()
            && matches!(
                action,
                "equip"
                    | "idle"
                    | "charge"
                    | "hold"
                    | "release"
                    | "slam"
                    | "attack"
                    | "use"
                    | "cast"
                    | "fishing"
                    | "reel"
            )
        {
            let mut defaults = definition["sequence"]["defaults"].clone();
            if !entry.is_null() {
                merge(&mut defaults, &entry);
            }
            entry = defaults;
        }
        entry
    }

    pub fn emotes(&self) -> Vec<(String, String)> {
        self.config["emotes"]["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some((
                    e["display_name"]
                        .as_str()
                        .or_else(|| e["id"].as_str())?
                        .to_owned(),
                    e["animation"].as_str()?.to_owned(),
                ))
            })
            .collect()
    }
}

/// The old resource-pack schema is still accepted by Moves 1.0.9. Internally
/// this port uses one canonical v2 tree, without copying pack configuration.
pub fn normalize(source: &Value) -> Result<Value, String> {
    if let Some(version) = source.get("format_version") {
        if version.as_u64() != Some(2) && version.as_str() != Some("2") {
            return Err("Unsupported Moves compatibility format (expected 2 or legacy)".into());
        }
        return Ok(source.clone());
    }
    if [
        "defaults",
        "item_groups",
        "items",
        "empty_hand",
        "cameras",
        "emotes",
    ]
    .iter()
    .any(|k| source.get(*k).is_some())
    {
        return Err("Moves v2 configuration needs format_version: 2".into());
    }
    let mut result = serde_json::json!({"format_version":2});
    if let Some(variables) = source.get("variables") {
        result["variables"] = variables.clone();
    }
    if let Some(static_use) = source.get("static_when_using") {
        result["defaults"]["static_when_using"] = static_use.clone();
    }
    legacy_actions(&mut result, source.get("default_animations"), "defaults");
    if let Some(empty) = source.get("emptyHand") {
        result["empty_hand"] = legacy_item(empty);
    }
    for (old, new) in [("item", "item_groups"), ("itemSpecific", "items")] {
        if let Some(groups) = source[old].as_object() {
            for (name, definition) in groups {
                let name = if new == "item_groups" {
                    name.to_ascii_uppercase()
                } else {
                    name.clone()
                };
                result[new][name] = legacy_item(definition);
            }
        }
    }
    Ok(result)
}
fn rename_clips(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(name) = object.remove("name") {
                object.insert("clip".into(), name);
            }
            for (_, v) in object {
                rename_clips(v);
            }
        }
        Value::Array(values) => {
            for v in values {
                rename_clips(v);
            }
        }
        _ => {}
    }
}
fn legacy_item(value: &Value) -> Value {
    let mut result = serde_json::json!({});
    for key in ["static_when_using", "camera", "better_combat"] {
        if let Some(v) = value.get(key) {
            result[key] = v.clone();
        }
    }
    let mut wrapper = serde_json::json!({});
    legacy_actions(&mut wrapper, value.get("customAnimation"), "item");
    if !wrapper["item"].is_null() {
        merge(&mut result, &wrapper["item"]);
    }
    result
}
fn legacy_actions(result: &mut Value, entries: Option<&Value>, destination: &str) {
    for entry in entries.and_then(Value::as_array).into_iter().flatten() {
        let Some(kind) = entry["type"].as_str() else {
            continue;
        };
        let mut entry = entry.clone();
        entry.as_object_mut().map(|o| o.remove("type"));
        rename_clips(&mut entry);
        match kind {
            "emotes" => merge(&mut result["emotes"], &entry),
            "proximity_camera" => merge(&mut result["cameras"]["proximity"], &entry),
            "bow_and_crossbow_camera_fix" => merge(&mut result["cameras"]["aiming"], &entry),
            "item_sequence" => merge(&mut result[destination]["sequence"], &entry),
            kind => {
                let kind = kind.strip_prefix("sequence_").unwrap_or(kind);
                let kind = if kind.eq_ignore_ascii_case("useitem") {
                    "use_item"
                } else {
                    kind
                };
                merge(&mut result[destination]["animations"][kind], &entry);
            }
        }
    }
}

pub fn item_group(id: &str) -> &'static str {
    let name = id.rsplit(':').next().unwrap_or(id);
    if name.is_empty() {
        "EMPTY"
    } else if name.ends_with("_pickaxe") {
        "PICKAXE"
    } else if name.ends_with("_axe") {
        "AXE"
    } else if name.ends_with("_shovel") {
        "SHOVEL"
    } else if name.ends_with("_hoe") {
        "HOE"
    } else if name.ends_with("_sword") {
        "SWORD"
    } else if name.ends_with("_spear") {
        "SPEAR"
    } else if name.ends_with("_bucket") {
        "FILLED_BUCKET"
    } else if name.ends_with("lantern") {
        "LANTERN"
    } else if name.ends_with("torch") {
        "TORCH"
    } else {
        match name {
            "bow" => "BOW",
            "crossbow" => "CROSSBOW",
            "trident" => "TRIDENT",
            "mace" => "MACE",
            "shield" => "SHIELD",
            "spyglass" => "SPYGLASS",
            "brush" => "BRUSH",
            "bundle" => "BUNDLE",
            "bucket" => "EMPTY_BUCKET",
            "fishing_rod" => "FISHING_ROD",
            "flint_and_steel" => "FLINT",
            "shears" => "SHEARS",
            "map" | "filled_map" => "MAP",
            "goat_horn" => "HORNS",
            "snowball" | "egg" | "ender_pearl" | "experience_bottle" | "splash_potion"
            | "lingering_potion" => "THROWABLE",
            _ => "GENERAL",
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::field_reassign_with_default,
    reason = "Tests evolve observation fixtures across several ticks"
)]
mod tests {
    use super::*;
    #[test]
    fn merges_defaults_without_losing_sibling_fields() {
        let mut p = Pack::default();
        p.config = serde_json::json!({"defaults":{"animations":{"attack":{"clip":"punch","retrigger_progress":50}}},
            "item_groups":{"SWORD":{"animations":{"attack":{"variants":["a","b"]}}}}});
        let e = p.entry("SWORD", "attack");
        assert_eq!(e["retrigger_progress"], 50);
        assert_eq!(e["variants"][1], "b");
        assert_eq!(item_group("minecraft:diamond_pickaxe"), "PICKAXE");
        assert_eq!(item_group("minecraft:water_bucket"), "FILLED_BUCKET");
    }
    #[test]
    fn missing_pack_is_not_an_error_or_embedded_fallback() {
        let p = Pack::load(Path::new("does-not-exist-moves-test")).unwrap();
        assert!(p.clips.is_empty());
        assert!(p.emotes().is_empty());
    }
}
