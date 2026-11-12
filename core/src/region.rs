//! A region and its file form (one entry under `regions:` in `regions.yml`).

use super::flags::{self, FlagValue, Group};
use super::geom::{BlockPos, Shape};
use super::yaml::Value;

pub const GLOBAL_ID: &str = "__global__";

/// The one doing something, as membership sees them.
#[derive(Clone, Debug, PartialEq)]
pub struct Subject {
    pub uuid: u128,
    pub name: String,
    /// Permission groups (`g:<group>` members). Horizon has none yet (GAPS.md).
    pub groups: Vec<String>,
}

/// `players`, `unique-ids` and `groups` of an owner or member list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Domain {
    pub players: Vec<String>,
    pub unique_ids: Vec<String>,
    pub groups: Vec<String>,
}

pub fn uuid_string(u: u128) -> String {
    let h = format!("{u:032x}");
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub fn parse_uuid(s: &str) -> Option<u128> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

impl Domain {
    pub fn is_empty(&self) -> bool {
        self.players.is_empty() && self.unique_ids.is_empty() && self.groups.is_empty()
    }
    pub fn size(&self) -> usize {
        self.players.len() + self.unique_ids.len() + self.groups.len()
    }
    pub fn contains(&self, s: &Subject) -> bool {
        let id = uuid_string(s.uuid);
        self.unique_ids.iter().any(|u| u.eq_ignore_ascii_case(&id))
            || self.players.iter().any(|p| p.eq_ignore_ascii_case(&s.name))
            || self
                .groups
                .iter()
                .any(|g| s.groups.iter().any(|sg| sg.eq_ignore_ascii_case(g)))
    }
    pub fn add_uuid(&mut self, u: u128) -> bool {
        let id = uuid_string(u);
        if self.unique_ids.iter().any(|x| x.eq_ignore_ascii_case(&id)) {
            return false;
        }
        self.unique_ids.push(id);
        true
    }
    pub fn add_name(&mut self, n: &str) -> bool {
        let n = n.to_ascii_lowercase();
        if self.players.contains(&n) {
            return false;
        }
        self.players.push(n);
        true
    }
    pub fn add_group(&mut self, g: &str) -> bool {
        let g = g.to_ascii_lowercase();
        if self.groups.contains(&g) {
            return false;
        }
        self.groups.push(g);
        true
    }
    /// Removes a name, a UUID or a `g:group`; true if something was removed.
    pub fn remove(&mut self, who: &str, uuid: Option<u128>) -> bool {
        let before = self.size();
        if let Some(g) = who.strip_prefix("g:") {
            self.groups.retain(|x| !x.eq_ignore_ascii_case(g));
        } else {
            self.players.retain(|x| !x.eq_ignore_ascii_case(who));
            let mut ids = vec![who.to_ascii_lowercase()];
            if let Some(u) = uuid.or_else(|| parse_uuid(who)) {
                ids.push(uuid_string(u));
            }
            self.unique_ids
                .retain(|x| !ids.iter().any(|i| i.eq_ignore_ascii_case(x)));
        }
        self.size() != before
    }
    pub fn clear(&mut self) {
        *self = Domain::default();
    }

    fn to_yaml(&self) -> Value {
        let mut m = Vec::new();
        let list = |v: &[String]| Value::List(v.iter().map(|s| Value::Str(s.clone())).collect());
        if !self.players.is_empty() {
            m.push(("players".into(), list(&self.players)));
        }
        if !self.unique_ids.is_empty() {
            m.push(("unique-ids".into(), list(&self.unique_ids)));
        }
        if !self.groups.is_empty() {
            m.push(("groups".into(), list(&self.groups)));
        }
        Value::Map(m)
    }

    fn from_yaml(v: Option<&Value>) -> Domain {
        let list = |k: &str| -> Vec<String> {
            v.and_then(|v| v.get(k))
                .and_then(Value::as_list)
                .map(|l| l.iter().filter_map(Value::to_text).collect())
                .unwrap_or_default()
        };
        Domain {
            players: list("players").into_iter().map(|p| p.to_ascii_lowercase()).collect(),
            unique_ids: list("unique-ids"),
            groups: list("groups"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub id: String,
    pub shape: Shape,
    pub priority: i32,
    pub parent: Option<String>,
    pub owners: Domain,
    pub members: Domain,
    /// Flag values in file order.
    pub flags: Vec<(String, FlagValue)>,
    /// `<flag>-group` entries.
    pub groups: Vec<(String, Group)>,
    /// Keys of the entry this port does not use, written back as read.
    pub extra: Vec<(String, Value)>,
    /// `-group` values that are not a known group, kept as read.
    pub raw_groups: Vec<(String, Value)>,
}

impl Region {
    pub fn new(id: &str, shape: Shape) -> Region {
        Region {
            id: id.to_ascii_lowercase(),
            shape,
            priority: 0,
            parent: None,
            owners: Domain::default(),
            members: Domain::default(),
            flags: Vec::new(),
            groups: Vec::new(),
            extra: Vec::new(),
            raw_groups: Vec::new(),
        }
    }

    pub fn is_global(&self) -> bool {
        self.id == GLOBAL_ID
    }

    /// The `__global__` region as `/rg flags` or `/rg info __global__` makes it on demand: nothing
    /// set on it. It is no region a player defined, so `/rg list` leaves it out until it has a
    /// flag, an owner, a member, a priority or a parent.
    pub fn is_blank_global(&self) -> bool {
        self.is_global()
            && self.flags.is_empty()
            && self.groups.is_empty()
            && self.owners.is_empty()
            && self.members.is_empty()
            && self.priority == 0
            && self.parent.is_none()
    }

    pub fn contains(&self, p: BlockPos) -> bool {
        self.shape.contains(p)
    }

    pub fn flag(&self, name: &str) -> Option<&FlagValue> {
        self.flags.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    pub fn set_flag(&mut self, name: &str, v: Option<FlagValue>) {
        match v {
            Some(v) => {
                if let Some(slot) = self.flags.iter_mut().find(|(n, _)| n == name) {
                    slot.1 = v;
                } else {
                    self.flags.push((name.to_owned(), v));
                }
            }
            None => {
                self.flags.retain(|(n, _)| n != name);
                self.groups.retain(|(n, _)| n != name);
            }
        }
    }

    /// The region group set for `flag`, if any.
    pub fn group(&self, name: &str) -> Option<Group> {
        self.groups.iter().find(|(n, _)| n == name).map(|(_, g)| *g)
    }

    pub fn set_group(&mut self, name: &str, g: Option<Group>) {
        self.groups.retain(|(n, _)| n != name);
        if let Some(g) = g {
            self.groups.push((name.to_owned(), g));
        }
    }

    /// The region's own owner/member standing of `s` (no parents).
    pub fn own_standing(&self, s: &Subject) -> (bool, bool) {
        (self.members.contains(s), self.owners.contains(s))
    }

    // -- file form ------------------------------------------------------------------------

    pub fn to_yaml(&self) -> Value {
        let mut m: Vec<(String, Value)> = Vec::new();
        let xyz = |p: BlockPos| {
            Value::Map(vec![
                ("x".into(), Value::Int(p.x as i64)),
                ("y".into(), Value::Int(p.y as i64)),
                ("z".into(), Value::Int(p.z as i64)),
            ])
        };
        match &self.shape {
            Shape::Cuboid { min, max } => {
                m.push(("min".into(), xyz(*min)));
                m.push(("max".into(), xyz(*max)));
            }
            Shape::Polygon { points, min_y, max_y } => {
                m.push(("min-y".into(), Value::Int(*min_y as i64)));
                m.push(("max-y".into(), Value::Int(*max_y as i64)));
                m.push((
                    "points".into(),
                    Value::List(
                        points
                            .iter()
                            .map(|p| {
                                Value::Map(vec![
                                    ("x".into(), Value::Int(p[0] as i64)),
                                    ("z".into(), Value::Int(p[1] as i64)),
                                ])
                            })
                            .collect(),
                    ),
                ));
            }
            Shape::Global => {}
        }
        m.push(("members".into(), self.members.to_yaml()));
        let mut flags: Vec<(String, Value)> =
            self.flags.iter().map(|(n, v)| (n.clone(), v.to_yaml())).collect();
        for (n, g) in &self.groups {
            flags.push((format!("{n}-group"), Value::Str(g.file_name().into())));
        }
        for (n, v) in &self.raw_groups {
            flags.push((format!("{n}-group"), v.clone()));
        }
        m.push(("flags".into(), Value::Map(flags)));
        m.push(("owners".into(), self.owners.to_yaml()));
        m.push(("type".into(), Value::Str(self.shape.type_name().into())));
        m.push(("priority".into(), Value::Int(self.priority as i64)));
        if let Some(p) = &self.parent {
            m.push(("parent".into(), Value::Str(p.clone())));
        }
        m.extend(self.extra.iter().cloned());
        Value::Map(m)
    }

    pub fn from_yaml(id: &str, v: &Value) -> Result<Region, String> {
        let ty = v
            .get("type")
            .and_then(Value::to_text)
            .unwrap_or_else(|| "cuboid".into())
            .to_ascii_lowercase();
        let corner = |k: &str| -> Result<BlockPos, String> {
            let c = v.get(k).ok_or(format!("region {id}: no {k}"))?;
            let n = |a: &str| {
                c.get(a)
                    .and_then(Value::as_f64)
                    .map(|f| f.floor() as i32)
                    .ok_or(format!("region {id}: bad {k}.{a}"))
            };
            Ok(BlockPos::new(n("x")?, n("y")?, n("z")?))
        };
        let shape = match ty.as_str() {
            "cuboid" => Shape::cuboid(corner("min")?, corner("max")?),
            "poly2d" => {
                let pts = v
                    .get("points")
                    .and_then(Value::as_list)
                    .ok_or(format!("region {id}: no points"))?;
                let mut points = Vec::new();
                for p in pts {
                    let x = p.get("x").and_then(Value::as_f64);
                    let z = p.get("z").and_then(Value::as_f64);
                    match (x, z) {
                        (Some(x), Some(z)) => points.push([x.floor() as i32, z.floor() as i32]),
                        _ => return Err(format!("region {id}: bad point")),
                    }
                }
                let y = |k: &str| v.get(k).and_then(Value::as_i64).map(|y| y as i32);
                Shape::polygon(points, y("min-y").unwrap_or(i32::MIN / 2), y("max-y").unwrap_or(i32::MAX / 2))
            }
            "global" => Shape::Global,
            other => return Err(format!("region {id}: unknown type {other}")),
        };
        let mut r = Region::new(id, shape);
        r.priority = v
            .get("priority")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        r.parent = v
            .get("parent")
            .and_then(Value::to_text)
            .filter(|p| !p.is_empty())
            .map(|p| p.to_ascii_lowercase());
        r.owners = Domain::from_yaml(v.get("owners"));
        r.members = Domain::from_yaml(v.get("members"));
        if let Some(fl) = v.get("flags").and_then(Value::as_map) {
            for (k, val) in fl {
                if let Some(base) = k.strip_suffix("-group")
                    && flags::lookup_exact(k).is_none()
                {
                    match val.to_text().as_deref().and_then(Group::parse) {
                        Some(g) => r.groups.push((base.to_owned(), g)),
                        None => r.raw_groups.push((base.to_owned(), val.clone())),
                    }
                    continue;
                }
                let def = flags::lookup_exact(k);
                let name = def.map(|d| d.name.to_owned()).unwrap_or_else(|| k.clone());
                r.flags.push((name, FlagValue::from_yaml(def, val)));
            }
        }
        const KNOWN: [&str; 11] = [
            "min", "max", "min-y", "max-y", "points", "members", "owners", "flags", "type",
            "priority", "parent",
        ];
        if let Some(entries) = v.as_map() {
            r.extra = entries
                .iter()
                .filter(|(k, _)| !KNOWN.contains(&k.as_str()))
                .cloned()
                .collect();
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::yaml;

    #[test]
    fn reads_and_writes_entry() {
        let t = "min: {x: 1730.0, y: 0.0, z: -169.0}\nmax: {x: 1742.0, y: 255.0, z: -158.0}\nmembers:\n    players: [player_a]\n    unique-ids: [00000000-0000-4000-8000-000000000001]\nflags: {use: allow, greeting: Welcome!, pvp: allow, pvp-group: MEMBERS, weird-flag: 3}\nowners:\n    groups: [admins]\ntype: cuboid\npriority: 4\n";
        let v = yaml::parse(t).unwrap();
        let r = Region::from_yaml("Test", &v).unwrap();
        assert_eq!(r.id, "test");
        assert_eq!(r.priority, 4);
        assert_eq!(r.group("pvp"), Some(Group::Members));
        assert_eq!(r.flag("greeting"), Some(&FlagValue::Str("Welcome!".into())));
        assert_eq!(r.flag("weird-flag"), Some(&FlagValue::Raw(Value::Int(3))));
        let s = Subject {
            uuid: parse_uuid("00000000-0000-4000-8000-000000000001").unwrap(),
            name: "someone".into(),
            groups: vec![],
        };
        assert!(r.members.contains(&s));
        assert!(!r.owners.contains(&s));
        let back = Region::from_yaml("test", &r.to_yaml()).unwrap();
        assert_eq!(back, r);
        assert!(r.contains(BlockPos::new(1742, 255, -158)));
    }

    #[test]
    fn poly_and_global() {
        let t = "min-y: 0\nmax-y: 64\npoints:\n- {x: 0, z: 0}\n- {x: 10, z: 0}\n- {x: 0, z: 10}\ntype: poly2d\npriority: 1\nparent: town\nmembers: {}\nowners: {}\nflags: {}\n";
        let r = Region::from_yaml("plot", &yaml::parse(t).unwrap()).unwrap();
        assert_eq!(r.parent.as_deref(), Some("town"));
        assert!(r.contains(BlockPos::new(2, 3, 2)));
        assert_eq!(Region::from_yaml("plot", &r.to_yaml()).unwrap(), r);
        let g = Region::from_yaml(GLOBAL_ID, &yaml::parse("type: global\nflags: {tnt: deny}\n").unwrap())
            .unwrap();
        assert!(g.is_global());
        assert!(!g.contains(BlockPos::new(0, 0, 0)));
    }

    #[test]
    fn uuids() {
        let u = 0x00000000000040008000000000000001_u128;
        assert_eq!(uuid_string(u), "00000000-0000-4000-8000-000000000001");
        assert_eq!(parse_uuid(&uuid_string(u)), Some(u));
    }
}
