//! Data-only facial libraries from selected resource packs. No embedded artwork.
use super::{FaceExpression, FaceSettings, Rgba, parse_color};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn key(name: &str) -> String {
    name.trim().to_lowercase().replace(' ', "_")
}
#[derive(Clone, Debug)]
pub struct FaceGrid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct FaceShape {
    pub closed: bool,
    pub on_white: bool,
    pub mirror: bool,
    pub color: Option<Rgba>,
    pub colors: [Option<Rgba>; 8],
    pub grids: Vec<FaceGrid>,
}
impl FaceShape {
    pub fn from_json(v: &Value) -> Result<Self, String> {
        let mut shape = Self {
            closed: v["closed"].as_bool().unwrap_or(false),
            on_white: v["on_white"].as_bool().unwrap_or(false),
            mirror: v["mirror"].as_bool().unwrap_or(true),
            color: parse_color(&v["color"]),
            colors: std::array::from_fn(|i| parse_color(&v["colors"][&(i + 2).to_string()])),
            grids: Vec::new(),
        };
        let grids = v["grids"].as_object().ok_or("eye shape has no grids")?;
        for (size, rows) in grids.iter().take(256) {
            let dimensions = size.to_ascii_lowercase();
            let (w, h) = dimensions.split_once('x').ok_or("grid size must be WxH")?;
            let w = w
                .trim()
                .parse::<usize>()
                .map_err(|_| "invalid grid width")?;
            let h = h
                .trim()
                .parse::<usize>()
                .map_err(|_| "invalid grid height")?;
            let rows = rows.as_array().ok_or("grid rows must be strings")?;
            if !(1..=16).contains(&w) || !(1..=16).contains(&h) || rows.len() != h {
                return Err(format!("invalid grid {size}"));
            }
            let mut cells = Vec::with_capacity(w * h);
            for row in rows {
                let row = row.as_str().ok_or("grid row must be a string")?;
                if row.chars().count() != w {
                    return Err(format!("incorrect row width in {size}"));
                }
                for c in row.chars() {
                    cells.push(if "#Xx1oO*".contains(c) {
                        1
                    } else if ('2'..='9').contains(&c)
                        && shape.colors[c as usize - '2' as usize].is_some()
                    {
                        c as u8 - b'0'
                    } else {
                        0
                    });
                }
            }
            shape.grids.push(FaceGrid {
                width: w,
                height: h,
                cells,
            });
        }
        if shape.grids.is_empty() {
            return Err("eye shape needs at least one grid".into());
        }
        Ok(shape)
    }
    pub fn minimum_rows(&self) -> usize {
        self.grids
            .iter()
            .map(|g| g.height)
            .min()
            .unwrap_or(2)
            .max(2)
    }
    pub fn sample(&self, u: f32, v: f32, side: usize, width: usize, height: usize) -> Option<Rgba> {
        if !(0.0..1.).contains(&u) || !(0.0..1.).contains(&v) {
            return None;
        }
        let grid = self.grids.iter().min_by_key(|grid| {
            (
                grid.width.abs_diff(width) * 16 + grid.height.abs_diff(height),
                std::cmp::Reverse(grid.width * grid.height),
            )
        })?;
        let x = (u * width as f32).floor() as isize;
        let x = if side == 1 && self.mirror {
            width as isize - 1 - x
        } else {
            x
        };
        let x = x + (grid.width as isize - width as isize) / 2;
        let y = (v * height as f32).floor() as isize + (grid.height as isize - height as isize) / 2;
        if x < 0 || y < 0 || x >= grid.width as isize || y >= grid.height as isize {
            return None;
        }
        let tone = grid.cells[y as usize * grid.width + x as usize];
        match tone {
            0 => None,
            1 => Some(self.color.unwrap_or([0; 4])),
            2..=9 => self.colors[(tone - 2) as usize],
            _ => None,
        }
    }
    pub fn to_json(&self) -> Value {
        let mut grids = serde_json::Map::new();
        for grid in &self.grids {
            let rows = grid
                .cells
                .chunks(grid.width)
                .map(|row| {
                    row.iter()
                        .map(|&c| {
                            if c == 0 {
                                '.'
                            } else if c == 1 {
                                '#'
                            } else {
                                char::from(b'0' + c)
                            }
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>();
            grids.insert(format!("{}x{}", grid.width, grid.height), json!(rows));
        }
        let colors = self
            .colors
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.map(|c| ((i + 2).to_string(), super::color_json(Some(c)))))
            .collect::<BTreeMap<_, _>>();
        json!({"closed":self.closed,"on_white":self.on_white,"mirror":self.mirror,
            "color":self.color.map_or(json!("line"),|c|super::color_json(Some(c))),"colors":colors,"grids":grids})
    }
}
#[derive(Clone, Debug, Default)]
pub struct FaceLibrary {
    pub shapes: BTreeMap<String, FaceShape>,
    pub looks: BTreeMap<String, FaceExpression>,
    pub triggers: BTreeMap<String, Value>,
    pub warnings: Vec<String>,
}
impl FaceLibrary {
    pub fn load(resources: &Path) -> Self {
        let root = resources.join("assets/punchy_moves/faces");
        let mut library = Self::default();
        let mut definitions = BTreeMap::new();
        let mut budget = 0_u64;
        for kind in ["shapes", "looks"] {
            let Ok(entries) = std::fs::read_dir(root.join(kind)) else {
                continue;
            };
            let mut paths = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .collect::<Vec<_>>();
            paths.sort();
            for path in paths.into_iter().take(512) {
                let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(key) else {
                    continue;
                };
                if path.extension().is_none_or(|ext| ext != "json") || path.is_symlink() {
                    continue;
                }
                let parsed = (|| {
                    use std::io::Read;
                    let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                    let size = file.metadata().map_err(|e| e.to_string())?.len();
                    if size > 1024 * 1024 || budget + size > 16 * 1024 * 1024 {
                        return Err("face library exceeds input limits".into());
                    }
                    let mut bytes = Vec::new();
                    file.take(1024 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    budget += bytes.len() as u64;
                    if bytes.len() > 1024 * 1024 {
                        return Err("face file exceeds limit".into());
                    }
                    serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string())
                })();
                match parsed {
                    Ok(value) if kind == "looks" => {
                        if value.is_object() {
                            definitions.insert(id, value);
                        } else {
                            library
                                .warnings
                                .push(format!("{}: look must be an object", path.display()));
                        }
                    }
                    Ok(value) => match FaceShape::from_json(&value) {
                        Ok(shape) => {
                            library.shapes.insert(id, shape);
                        }
                        Err(e) => library.warnings.push(format!("{}: {e}", path.display())),
                    },
                    Err(e) => library.warnings.push(format!("{}: {e}", path.display())),
                }
            }
        }
        fn resolve(
            id: &str,
            definitions: &BTreeMap<String, Value>,
            resolved: &mut BTreeMap<String, FaceExpression>,
            stack: &mut BTreeSet<String>,
        ) -> Result<FaceExpression, String> {
            if let Some(look) = resolved.get(id) {
                return Ok(look.clone());
            }
            let Some(value) = definitions.get(id) else {
                return if FaceExpression::known(id) {
                    Ok(FaceExpression::named(id))
                } else {
                    Err(format!("unknown inherited face look: {id}"))
                };
            };
            if stack.len() >= 32 || !stack.insert(id.into()) {
                return Err(format!("cyclic/deep face look inheritance: {id}"));
            }
            let parent = value["extends"].as_str().map(key);
            let mut merged = if let Some(parent) = parent {
                resolve(&parent, definitions, resolved, stack)?.to_json()
            } else {
                json!({})
            };
            crate::moves_pack::merge(&mut merged, value);
            if value.get("blink").is_some() {
                merged.as_object_mut().unwrap().remove("blinks");
                merged.as_object_mut().unwrap().remove("blink_pace");
            }
            let look = FaceExpression::from_json(id, &merged);
            stack.remove(id);
            resolved.insert(id.into(), look.clone());
            Ok(look)
        }
        for id in definitions.keys() {
            if let Err(e) = resolve(id, &definitions, &mut library.looks, &mut BTreeSet::new()) {
                library.warnings.push(e);
            }
        }
        let trigger_path = root.join("triggers.json");
        if let Ok(file) = std::fs::File::open(&trigger_path) {
            use std::io::Read;
            match serde_json::from_reader::<_, Value>(file.take(1024 * 1024)) {
                Ok(Value::Object(triggers)) => library
                    .triggers
                    .extend(triggers.into_iter().map(|(id, v)| (key(&id), v))),
                Ok(_) => library
                    .warnings
                    .push("face triggers must be an object".into()),
                Err(e) => library.warnings.push(format!("face triggers: {e}")),
            }
        }
        library
    }
    pub fn apply(&self, settings: &mut FaceSettings) {
        for (id, shape) in &self.shapes {
            settings
                .shapes
                .entry(id.clone())
                .or_insert_with(|| shape.clone());
        }
        for (id, look) in &self.looks {
            settings
                .custom_looks
                .entry(id.clone())
                .or_insert_with(|| look.clone());
        }
        for (id, value) in &self.triggers {
            if let Some(look) = value.as_str().or_else(|| value["look"].as_str()) {
                settings
                    .trigger_looks
                    .entry(id.clone())
                    .or_insert_with(|| key(look));
            }
            if let Some(priority) = value["priority"].as_i64() {
                settings
                    .trigger_priorities
                    .entry(id.clone())
                    .or_insert(priority.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_grid_preserves_tones_and_mirrors_at_half_pixel_resolution() {
        let value = json!({"closed": false, "color": "#123456", "colors": {"2": "#ABCDEF"},
            "grids": {"4X2": ["#2..", ".##."]}});
        let shape = FaceShape::from_json(&value).unwrap();
        assert_eq!(
            shape.sample(0.125, 0.25, 0, 4, 2),
            Some([0x12, 0x34, 0x56, 255])
        );
        assert_eq!(
            shape.sample(0.375, 0.25, 0, 4, 2),
            Some([0xab, 0xcd, 0xef, 255])
        );
        assert_eq!(
            shape.sample(0.875, 0.25, 1, 4, 2),
            shape.sample(0.125, 0.25, 0, 4, 2)
        );
        let decoded = FaceShape::from_json(&shape.to_json()).unwrap();
        assert_eq!(decoded.grids[0].cells, shape.grids[0].cells);
        assert!(FaceShape::from_json(&json!({"grids":{"16x2":["#","#"]}})).is_err());
    }
    #[test]
    fn centering_a_smaller_grid_uses_truncation_towards_zero() {
        let shape = FaceShape::from_json(&json!({"grids":{"1x2":["#","#"]}})).unwrap();
        assert!(shape.sample(0.25, 0.25, 0, 2, 2).is_some());
        assert!(shape.sample(0.75, 0.25, 0, 2, 2).is_none());
    }
}
