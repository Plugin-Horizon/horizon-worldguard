//! The regions of one world, read from and written to `regions.yml`, with a chunk index for
//! point queries.

use super::geom::{BlockPos, Shape};
use super::region::{GLOBAL_ID, Region};
use super::yaml::{self, Value};
use std::collections::{BTreeMap, HashMap};

const HEADER: &str = "#\n# WorldGuard regions file\n#\n# This file is generated. Edit it only while the server is stopped, or run\n# /rg load afterwards. A single mistyped character can make it unreadable.\n#\n# Keep backups.\n#\n";

/// Regions whose box spans more chunks than this are scanned instead of indexed.
const MAX_INDEXED_CHUNKS: i64 = 4096;

#[derive(Debug, Default)]
pub struct World {
    pub name: String,
    regions: BTreeMap<String, Region>,
    /// Chunk -> ids of regions touching it.
    index: HashMap<(i32, i32), Vec<String>>,
    /// Ids of regions too large to index.
    large: Vec<String>,
    pub dirty: bool,
}

#[derive(Debug, PartialEq)]
pub enum RemoveMode {
    /// Refuse when the region has children.
    Refuse,
    /// Children lose their parent (`-u`).
    Unset,
    /// Children are removed too (`-f`).
    Cascade,
}

impl World {
    pub fn new(name: &str) -> World {
        World {
            name: name.to_owned(),
            ..World::default()
        }
    }

    /// Reads a `regions.yml` text. Regions that cannot be read are reported and skipped; a file
    /// that is not YAML at all is an error (nothing is loaded, so nothing is overwritten).
    pub fn load(name: &str, text: &str) -> Result<(World, Vec<String>), String> {
        let doc = yaml::parse(text).map_err(|e| e.to_string())?;
        let mut w = World::new(name);
        let mut problems = Vec::new();
        if let Some(regions) = doc.get("regions").and_then(Value::as_map) {
            for (id, v) in regions {
                match Region::from_yaml(id, v) {
                    Ok(r) => {
                        w.regions.insert(r.id.clone(), r);
                    }
                    Err(e) => problems.push(e),
                }
            }
        }
        // Parents that do not exist or loop are dropped, as a loader must.
        let ids: Vec<String> = w.regions.keys().cloned().collect();
        for id in ids {
            let parent = w.regions[&id].parent.clone();
            if let Some(p) = parent
                && (!w.regions.contains_key(&p) || w.would_cycle(&id, &p))
            {
                problems.push(format!("region {id}: parent {p} missing or circular, dropped"));
                w.regions.get_mut(&id).unwrap().parent = None;
            }
        }
        w.reindex();
        Ok((w, problems))
    }

    pub fn save(&self) -> String {
        let regions: Vec<(String, Value)> = self
            .regions
            .values()
            .map(|r| (r.id.clone(), r.to_yaml()))
            .collect();
        let doc = Value::Map(vec![("regions".into(), Value::Map(regions))]);
        format!("{HEADER}{}", yaml::emit(&doc, 4))
    }

    pub fn get(&self, id: &str) -> Option<&Region> {
        self.regions.get(&id.to_ascii_lowercase())
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Region> {
        self.dirty = true;
        self.regions.get_mut(&id.to_ascii_lowercase())
    }

    /// The `__global__` region, created on first use.
    pub fn global_mut(&mut self) -> &mut Region {
        self.dirty = true;
        self.regions
            .entry(GLOBAL_ID.to_owned())
            .or_insert_with(|| Region::new(GLOBAL_ID, Shape::Global))
    }

    pub fn global(&self) -> Option<&Region> {
        self.regions.get(GLOBAL_ID)
    }

    pub fn all(&self) -> impl Iterator<Item = &Region> {
        self.regions.values()
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Adds or replaces a region (keeping a replaced region's children pointing at it).
    pub fn put(&mut self, r: Region) {
        self.regions.insert(r.id.clone(), r);
        self.dirty = true;
        self.reindex();
    }

    /// Call after changing a region's shape through `get_mut`.
    pub fn reshaped(&mut self) {
        self.dirty = true;
        self.reindex();
    }

    pub fn children(&self, id: &str) -> Vec<String> {
        let id = id.to_ascii_lowercase();
        self.regions
            .values()
            .filter(|r| r.parent.as_deref() == Some(id.as_str()))
            .map(|r| r.id.clone())
            .collect()
    }

    /// Removes `id`; returns the ids removed, or an error naming the children.
    pub fn remove(&mut self, id: &str, mode: RemoveMode) -> Result<Vec<String>, String> {
        let id = id.to_ascii_lowercase();
        if !self.regions.contains_key(&id) {
            return Err(format!("No region named '{id}'."));
        }
        let kids = self.children(&id);
        let mut removed = vec![id.clone()];
        if !kids.is_empty() {
            match mode {
                RemoveMode::Refuse => {
                    return Err(format!(
                        "Region '{id}' has child regions ({}): use -f to remove them too, or -u to unset their parent.",
                        kids.join(", ")
                    ));
                }
                RemoveMode::Unset => {
                    for k in &kids {
                        if let Some(r) = self.regions.get_mut(k) {
                            r.parent = None;
                        }
                    }
                }
                RemoveMode::Cascade => {
                    for k in kids {
                        if let Ok(more) = self.remove(&k, RemoveMode::Cascade) {
                            removed.extend(more);
                        }
                    }
                }
            }
        }
        self.regions.remove(&id);
        self.dirty = true;
        self.reindex();
        Ok(removed)
    }

    fn would_cycle(&self, child: &str, parent: &str) -> bool {
        let mut at = Some(parent.to_owned());
        let mut steps = 0;
        while let Some(p) = at {
            if p == child {
                return true;
            }
            steps += 1;
            if steps > self.regions.len() + 1 {
                return true;
            }
            at = self.regions.get(&p).and_then(|r| r.parent.clone());
        }
        false
    }

    pub fn set_parent(&mut self, id: &str, parent: Option<&str>) -> Result<(), String> {
        let id = id.to_ascii_lowercase();
        if !self.regions.contains_key(&id) {
            return Err(format!("No region named '{id}'."));
        }
        let parent = parent.map(str::to_ascii_lowercase);
        if let Some(p) = &parent {
            if p == GLOBAL_ID || id == GLOBAL_ID {
                return Err("The global region cannot have or be a parent.".into());
            }
            if !self.regions.contains_key(p) {
                return Err(format!("No region named '{p}'."));
            }
            if self.would_cycle(&id, p) {
                return Err("That would make a circular parent chain.".into());
            }
        }
        self.regions.get_mut(&id).unwrap().parent = parent;
        self.dirty = true;
        Ok(())
    }

    /// The parent chain of `id`, starting with its parent.
    pub fn ancestors(&self, id: &str) -> Vec<&Region> {
        let mut out = Vec::new();
        let mut at = self.regions.get(id).and_then(|r| r.parent.clone());
        while let Some(p) = at {
            let Some(r) = self.regions.get(&p) else { break };
            if out.iter().any(|o: &&Region| o.id == r.id) {
                break;
            }
            out.push(r);
            at = r.parent.clone();
        }
        out
    }

    fn reindex(&mut self) {
        self.index.clear();
        self.large.clear();
        for r in self.regions.values() {
            let Some((lo, hi)) = r.shape.bounds() else { continue };
            let (cx0, cz0, cx1, cz1) = (lo.x >> 4, lo.z >> 4, hi.x >> 4, hi.z >> 4);
            let n = (cx1 as i64 - cx0 as i64 + 1) * (cz1 as i64 - cz0 as i64 + 1);
            if n > MAX_INDEXED_CHUNKS {
                self.large.push(r.id.clone());
                continue;
            }
            for cx in cx0..=cx1 {
                for cz in cz0..=cz1 {
                    self.index.entry((cx, cz)).or_default().push(r.id.clone());
                }
            }
        }
    }

    /// Regions with an area that contain `p` (not the global region).
    pub fn at(&self, p: BlockPos) -> Vec<&Region> {
        let mut out: Vec<&Region> = Vec::new();
        let chunk = (p.x >> 4, p.z >> 4);
        let ids = self.index.get(&chunk).into_iter().flatten().chain(self.large.iter());
        for id in ids {
            if let Some(r) = self.regions.get(id)
                && r.contains(p)
                && !out.iter().any(|o| o.id == r.id)
            {
                out.push(r);
            }
        }
        out
    }

    /// Regions with an area that share a block with `shape`.
    pub fn intersecting(&self, shape: &Shape) -> Vec<&Region> {
        self.regions
            .values()
            .filter(|r| !matches!(r.shape, Shape::Global) && r.shape.intersects(shape))
            .collect()
    }
}

/// A valid region id (`ProtectedRegion.isValidId`): letters, digits and `_ , ' - + /`, at least
/// one character.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_,'-+/".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::flags::FlagValue;

    const FILE: &str = "regions:\n    town:\n        min: {x: 0, y: 0, z: 0}\n        max: {x: 100, y: 255, z: 100}\n        members: {}\n        flags: {pvp: deny}\n        owners: {}\n        type: cuboid\n        priority: 1\n    plot:\n        min: {x: 10, y: 0, z: 10}\n        max: {x: 20, y: 255, z: 20}\n        members: {}\n        flags: {}\n        owners: {}\n        type: cuboid\n        priority: 2\n        parent: town\n    __global__:\n        members: {}\n        flags: {tnt: deny}\n        owners: {}\n        type: global\n        priority: 0\n";

    #[test]
    fn load_query_save() {
        let (w, problems) = World::load("world", FILE).unwrap();
        assert!(problems.is_empty());
        assert_eq!(w.len(), 3);
        let here: Vec<_> = w.at(BlockPos::new(15, 64, 15)).iter().map(|r| r.id.clone()).collect();
        assert_eq!(here.len(), 2);
        assert!(w.at(BlockPos::new(-1, 64, 0)).is_empty());
        assert_eq!(w.ancestors("plot")[0].id, "town");
        let text = w.save();
        let (again, _) = World::load("world", &text).unwrap();
        assert_eq!(again.get("plot"), w.get("plot"));
        assert_eq!(again.global().unwrap().flag("tnt"), Some(&FlagValue::State(false)));
    }

    #[test]
    fn parents_and_removal() {
        let (mut w, _) = World::load("world", FILE).unwrap();
        assert!(w.set_parent("town", Some("plot")).is_err());
        assert!(w.remove("town", RemoveMode::Refuse).is_err());
        assert_eq!(w.remove("town", RemoveMode::Unset).unwrap(), vec!["town".to_string()]);
        assert_eq!(w.get("plot").unwrap().parent, None);
        let (mut w, _) = World::load("world", FILE).unwrap();
        let mut gone = w.remove("town", RemoveMode::Cascade).unwrap();
        gone.sort();
        assert_eq!(gone, vec!["plot".to_string(), "town".to_string()]);
    }

    #[test]
    fn broken_parent_is_dropped() {
        let t = "regions:\n    a:\n        min: {x: 0, y: 0, z: 0}\n        max: {x: 1, y: 1, z: 1}\n        type: cuboid\n        parent: b\n    b:\n        min: {x: 0, y: 0, z: 0}\n        max: {x: 1, y: 1, z: 1}\n        type: cuboid\n        parent: a\n";
        let (w, problems) = World::load("w", t).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(w.get("a").unwrap().parent.is_none() || w.get("b").unwrap().parent.is_none());
        assert!(valid_id("my_plot-1"));
        assert!(!valid_id("bad id"));
    }
}
