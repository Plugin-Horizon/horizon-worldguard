//! Region flags: the names, their value types and default region groups (from the public flag
//! documentation), and their values as written in region files and typed in commands.

use super::yaml::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    State,
    Bool,
    Str,
    Int,
    Double,
    Location,
    /// A set of strings (`blocked-cmds`), kept in the order given.
    StrSet,
    /// A set of entity types (`deny-spawn`).
    EntitySet,
    GameMode,
    Weather,
}

/// Region groups: who a flag applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    All,
    Members,
    Owners,
    NonMembers,
    NonOwners,
    None,
}

impl Group {
    /// `RegionGroupFlag`: the enum names (`NON_MEMBERS`, any case) and the words `detectValue` knows.
    pub fn parse(s: &str) -> Option<Group> {
        match s.to_ascii_uppercase().as_str() {
            "MEMBERS" => return Some(Group::Members),
            "OWNERS" => return Some(Group::Owners),
            "NON_MEMBERS" => return Some(Group::NonMembers),
            "NON_OWNERS" => return Some(Group::NonOwners),
            "ALL" => return Some(Group::All),
            "NONE" => return Some(Group::None),
            _ => {}
        }
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "members" | "member" => Group::Members,
            "owners" | "owner" => Group::Owners,
            "nonowners" | "nonowner" => Group::NonOwners,
            "nonmembers" | "nonmember" => Group::NonMembers,
            "everyone" | "anyone" | "all" => Group::All,
            "none" | "noone" | "deny" => Group::None,
            _ => return None,
        })
    }
    /// As written in region files (`pvp-group: MEMBERS`).
    pub fn file_name(self) -> &'static str {
        match self {
            Group::All => "ALL",
            Group::Members => "MEMBERS",
            Group::Owners => "OWNERS",
            Group::NonMembers => "NON_MEMBERS",
            Group::NonOwners => "NON_OWNERS",
            Group::None => "NONE",
        }
    }
    pub fn command_name(self) -> &'static str {
        match self {
            Group::All => "all",
            Group::Members => "members",
            Group::Owners => "owners",
            Group::NonMembers => "nonmembers",
            Group::NonOwners => "nonowners",
            Group::None => "none",
        }
    }
    /// Whether a subject with this standing in the region is in the group.
    pub fn contains(self, member: bool, owner: bool) -> bool {
        match self {
            Group::All => true,
            Group::Members => member || owner,
            Group::Owners => owner,
            Group::NonMembers => !(member || owner),
            Group::NonOwners => !owner,
            Group::None => false,
        }
    }
}

pub struct FlagDef {
    pub name: &'static str,
    pub kind: Kind,
    pub default_group: Group,
    /// Protection-related: checked together with `build`, falling back to membership.
    pub protection: bool,
    /// The value when no region sets it (state flags), None: no default.
    pub default_allow: Option<bool>,
}

const fn def(name: &'static str, kind: Kind) -> FlagDef {
    FlagDef {
        name,
        kind,
        default_group: Group::All,
        protection: false,
        default_allow: None,
    }
}
const fn prot(name: &'static str) -> FlagDef {
    FlagDef {
        name,
        kind: Kind::State,
        default_group: Group::All,
        protection: true,
        default_allow: None,
    }
}
/// The `build` flag: a protection flag that is allowed when nothing is protecting.
const fn build_flag() -> FlagDef {
    FlagDef {
        name: "build",
        kind: Kind::State,
        default_group: Group::All,
        protection: true,
        default_allow: Some(true),
    }
}
/// A state flag that is allowed when no region sets it.
const fn on(name: &'static str) -> FlagDef {
    FlagDef {
        name,
        kind: Kind::State,
        default_group: Group::All,
        protection: false,
        default_allow: Some(true),
    }
}
const fn grouped(name: &'static str, kind: Kind, group: Group, default_allow: Option<bool>) -> FlagDef {
    FlagDef {
        name,
        kind,
        default_group: group,
        protection: false,
        default_allow,
    }
}

use Kind::*;

/// Every built-in flag, in WorldGuard's registration order (the order of `/rg flags`).
/// `prot` flags default to unset (checked through the build query), `on` flags to allow.
pub static FLAGS: &[FlagDef] = &[
    // Overrides
    def("passthrough", State),
    grouped("nonplayer-protection-domains", StrSet, Group::None, None),
    // Protection-related
    build_flag(),
    prot("block-break"),
    prot("block-place"),
    prot("use"),
    prot("interact"),
    prot("damage-animals"),
    prot("pvp"),
    prot("sleep"),
    prot("respawn-anchors"),
    prot("tnt"),
    prot("chest-access"),
    prot("vehicle-place"),
    prot("vehicle-destroy"),
    prot("lighter"),
    prot("ride"),
    prot("potion-splash"),
    prot("item-frame-rotation"),
    prot("block-trampling"),
    prot("firework-damage"),
    prot("use-anvil"),
    prot("use-dripleaf"),
    prot("wind-charge-burst"),
    on("item-pickup"),
    on("item-drop"),
    on("exp-drops"),
    // Mobs, fire, explosions
    on("mob-damage"),
    on("creeper-explosion"),
    on("enderdragon-block-damage"),
    on("ghast-fireball"),
    on("other-explosion"),
    on("breeze-charge-explosion"),
    on("wither-damage"),
    on("enderman-grief"),
    on("snowman-trails"),
    on("ravager-grief"),
    on("entity-painting-destroy"),
    on("entity-item-frame-destroy"),
    on("mob-spawning"),
    def("deny-spawn", EntitySet),
    // Natural events
    on("pistons"),
    on("fire-spread"),
    on("lava-fire"),
    on("lightning"),
    on("snow-fall"),
    on("snow-melt"),
    on("ice-form"),
    on("ice-melt"),
    on("frosted-ice-melt"),
    prot("frosted-ice-form"),
    on("mushroom-growth"),
    on("leaf-decay"),
    on("grass-growth"),
    on("mycelium-spread"),
    on("vine-growth"),
    on("rock-growth"),
    on("sculk-growth"),
    on("crop-growth"),
    on("soil-dry"),
    on("coral-fade"),
    on("copper-fade"),
    on("water-flow"),
    on("lava-flow"),
    on("moisture-change"),
    on("lava-harden"),
    def("weather-lock", Weather),
    def("time-lock", Str),
    // Chat and commands
    on("send-chat"),
    on("receive-chat"),
    def("blocked-cmds", StrSet),
    def("allowed-cmds", StrSet),
    // Teleport and spawn
    def("teleport", Location),
    grouped("spawn", Location, Group::Members, None),
    def("teleport-message", Str),
    // Player state
    def("invincible", State),
    on("fall-damage"),
    on("natural-health-regen"),
    on("natural-hunger-drain"),
    // Movement
    grouped("entry", State, Group::NonMembers, Some(true)),
    grouped("exit", State, Group::NonMembers, Some(true)),
    def("exit-override", Bool),
    on("exit-via-teleport"),
    on("enderpearl"),
    on("chorus-fruit-teleport"),
    def("greeting", Str),
    def("farewell", Str),
    def("greeting-title", Str),
    def("farewell-title", Str),
    def("notify-enter", Bool),
    def("notify-leave", Bool),
    def("game-mode", GameMode),
    def("heal-delay", Int),
    def("heal-amount", Int),
    def("heal-min-health", Double),
    def("heal-max-health", Double),
    def("feed-delay", Int),
    def("feed-amount", Int),
    def("feed-min-hunger", Int),
    def("feed-max-hunger", Int),
    // Deny messages
    def("deny-message", Str),
    def("entry-deny-message", Str),
    def("exit-deny-message", Str),
];

/// WorldGuard's `Flags.fuzzyMatchFlag`: dashes ignored, case ignored.
pub fn lookup(name: &str) -> Option<&'static FlagDef> {
    let n: String = name.replace('-', "").to_ascii_lowercase();
    FLAGS.iter().find(|f| f.name.replace('-', "") == n)
}

/// The flag with exactly this name (region files).
pub fn lookup_exact(name: &str) -> Option<&'static FlagDef> {
    let n = name.to_ascii_lowercase();
    FLAGS.iter().find(|f| f.name == n)
}

/// The default `deny-message` (red bold "Hey!" and grey text).
pub const DEFAULT_DENY_MESSAGE: &str = "&c&lHey!&7 Sorry, but you can't %what% here.";
pub const DEFAULT_ENTRY_DENY_MESSAGE: &str = "&c&lHey!&7 You are not permitted to enter this area.";
pub const DEFAULT_EXIT_DENY_MESSAGE: &str = "&c&lHey!&7 You are not permitted to leave this area.";
pub const DEFAULT_TELEPORT_MESSAGE: &str = "&dTeleported you to the region '%id%'.";

#[derive(Clone, Debug, PartialEq)]
pub enum FlagValue {
    State(bool),
    Bool(bool),
    Str(String),
    Int(i64),
    Double(f64),
    /// `{world, x, y, z, yaw, pitch}` as in the file.
    Location(Location),
    Set(Vec<String>),
    /// A flag this port does not know: kept as read, written back unchanged.
    Raw(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub world: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

impl FlagValue {
    pub fn as_state(&self) -> Option<bool> {
        match self {
            FlagValue::State(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FlagValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn to_yaml(&self) -> Value {
        match self {
            FlagValue::State(b) => Value::Str(if *b { "allow" } else { "deny" }.into()),
            FlagValue::Bool(b) => Value::Bool(*b),
            FlagValue::Str(s) => Value::Str(s.clone()),
            FlagValue::Int(i) => Value::Int(*i),
            FlagValue::Double(d) => Value::Float(*d),
            FlagValue::Location(l) => Value::Map(vec![
                ("world".into(), Value::Str(l.world.clone())),
                ("x".into(), Value::Float(l.pos[0])),
                ("y".into(), Value::Float(l.pos[1])),
                ("z".into(), Value::Float(l.pos[2])),
                ("yaw".into(), Value::Float(l.yaw as f64)),
                ("pitch".into(), Value::Float(l.pitch as f64)),
            ]),
            FlagValue::Set(items) => Value::List(items.iter().map(|s| Value::Str(s.clone())).collect()),
            FlagValue::Raw(v) => v.clone(),
        }
    }

    /// Reads a value from a region file; an unreadable one is kept raw.
    pub fn from_yaml(def: Option<&FlagDef>, v: &Value) -> FlagValue {
        let Some(def) = def else {
            return FlagValue::Raw(v.clone());
        };
        let parsed = match def.kind {
            State => v.to_text().and_then(|t| parse_state(&t)).map(FlagValue::State),
            Bool => match v {
                Value::Bool(b) => Some(FlagValue::Bool(*b)),
                other => other.to_text().and_then(|t| parse_bool(&t)).map(FlagValue::Bool),
            },
            Str | GameMode | Weather => v.to_text().map(FlagValue::Str),
            Int => v.as_i64().map(FlagValue::Int),
            Double => v.as_f64().map(FlagValue::Double),
            Location => (|| {
                Some(FlagValue::Location(super::flags::Location {
                    world: v.get("world").and_then(Value::to_text).unwrap_or_default(),
                    pos: [
                        v.get("x")?.as_f64()?,
                        v.get("y")?.as_f64()?,
                        v.get("z")?.as_f64()?,
                    ],
                    yaw: v.get("yaw").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                    pitch: v.get("pitch").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                }))
            })(),
            StrSet | EntitySet => match v {
                Value::List(items) => Some(FlagValue::Set(
                    items.iter().filter_map(Value::to_text).collect(),
                )),
                Value::Null => Some(FlagValue::Set(Vec::new())),
                other => other.to_text().map(|t| FlagValue::Set(split_set(&t))),
            },
        };
        parsed.unwrap_or_else(|| FlagValue::Raw(v.clone()))
    }

    /// Parses what a player typed after `/rg flag <id> <flag>`, as the flag's `parseInput` does.
    /// `Ok(None)` is a value that unsets the flag (`none` for state flags). `here` is the
    /// sender's location (`here`, and the world of `x,y,z`).
    pub fn parse_input(
        def: &FlagDef,
        input: &str,
        here: Option<&Location>,
    ) -> Result<Option<FlagValue>, String> {
        match def.kind {
            State => {
                if input.eq_ignore_ascii_case("allow") {
                    Ok(Some(FlagValue::State(true)))
                } else if input.eq_ignore_ascii_case("deny") {
                    Ok(Some(FlagValue::State(false)))
                } else if input.eq_ignore_ascii_case("none") {
                    Ok(None)
                } else {
                    Err(format!("Expected none/allow/deny but got '{input}'"))
                }
            }
            Bool => parse_bool(input)
                .map(|b| Some(FlagValue::Bool(b)))
                .ok_or_else(|| format!("Not a yes/no value: {input}")),
            Str => Ok(Some(FlagValue::Str(string_input(input)))),
            Int => input
                .parse::<i32>()
                .map(|i| Some(FlagValue::Int(i as i64)))
                .map_err(|_| format!("Not a number: {input}")),
            Double => input
                .parse::<f64>()
                .map(|d| Some(FlagValue::Double(d)))
                .map_err(|_| format!("Not a number: {input}")),
            GameMode => {
                let key = input.trim().to_ascii_lowercase();
                let bare = key.strip_prefix("minecraft:").unwrap_or(&key);
                match bare {
                    "survival" | "creative" | "adventure" | "spectator" => {
                        Ok(Some(FlagValue::Str(format!("minecraft:{bare}"))))
                    }
                    _ => Err(format!("Unknown game mode: {key}")),
                }
            }
            Weather => {
                let key = input.trim().to_ascii_lowercase();
                let bare = key.strip_prefix("minecraft:").unwrap_or(&key);
                match bare {
                    "clear" | "rain" | "thunder" => Ok(Some(FlagValue::Str(format!("minecraft:{bare}")))),
                    _ => Err(format!("Unknown weather type: {key}")),
                }
            }
            Location => {
                let Some(me) = here else {
                    return Err("Expected 'here' or x,y,z.".to_owned());
                };
                if input.eq_ignore_ascii_case("here") {
                    return Ok(Some(FlagValue::Location(me.clone())));
                }
                let split: Vec<&str> = input.split(',').collect();
                if split.len() >= 3 {
                    let num = |i: usize| split.get(i).and_then(|s| s.trim().parse::<f64>().ok());
                    if let (Some(x), Some(y), Some(z)) = (num(0), num(1), num(2)) {
                        let yaw = if split.len() < 4 { Some(0.0) } else { num(3) };
                        let pitch = if split.len() < 5 { Some(0.0) } else { num(4) };
                        if let (Some(yaw), Some(pitch)) = (yaw, pitch) {
                            return Ok(Some(FlagValue::Location(super::flags::Location {
                                world: me.world.clone(),
                                pos: [x, y, z],
                                yaw: yaw as f32,
                                pitch: pitch as f32,
                            })));
                        }
                    }
                }
                Err("Expected 'here' or x,y,z.".to_owned())
            }
            StrSet => {
                if input.is_empty() {
                    return Ok(Some(FlagValue::Set(Vec::new())));
                }
                let mut out: Vec<String> = Vec::new();
                for part in input.split(',') {
                    let t = part.trim();
                    let cmd = if t.starts_with('/') { t.to_owned() } else { format!("/{t}") };
                    let cmd = cmd.to_lowercase();
                    if !out.contains(&cmd) {
                        out.push(cmd);
                    }
                }
                Ok(Some(FlagValue::Set(out)))
            }
            EntitySet => {
                if input.is_empty() {
                    return Ok(Some(FlagValue::Set(Vec::new())));
                }
                let mut out: Vec<String> = Vec::new();
                for part in input.split(',') {
                    let key = part.trim().to_ascii_lowercase();
                    let ok = !key.is_empty()
                        && key
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_:./-".contains(c));
                    if !ok {
                        return Err(format!("Unknown entity type: {key}"));
                    }
                    let id = entity_id(&key);
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
                Ok(Some(FlagValue::Set(out)))
            }
        }
    }

    /// What Java's `String.valueOf(value)` shows for `/rg info` and the flag lists.
    pub fn display(&self) -> String {
        match self {
            FlagValue::State(b) => if *b { "ALLOW" } else { "DENY" }.into(),
            FlagValue::Bool(b) => b.to_string(),
            FlagValue::Str(s) => s.clone(),
            FlagValue::Int(i) => i.to_string(),
            FlagValue::Double(d) => format!("{d:?}"),
            FlagValue::Location(l) => format!(
                "Location{{world={}, x={:?}, y={:?}, z={:?}, yaw={:?}, pitch={:?}}}",
                l.world, l.pos[0], l.pos[1], l.pos[2], l.yaw, l.pitch
            ),
            FlagValue::Set(s) => format!("[{}]", s.join(", ")),
            FlagValue::Raw(v) => super::yaml::flow(v),
        }
    }
}

/// `StringFlag.parseInput`: `\n` becomes a line break (`\\n` stays a literal `\n`), then the
/// colour macros.
pub fn string_input(input: &str) -> String {
    let mut once = String::new();
    let mut prev = '\0';
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && prev != '\\' && chars.peek() == Some(&'n') {
            chars.next();
            once.push('\n');
            prev = 'n';
            continue;
        }
        once.push(c);
        prev = c;
    }
    super::text::color_macros(&once.replace("\\\\n", "\\n"))
}

/// `BooleanFlag.parseInput`.
pub fn parse_bool(t: &str) -> Option<bool> {
    match t.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// `StateFlag.unmarshal`: only `allow` and `deny` are states.
pub fn parse_state(t: &str) -> Option<bool> {
    match t.trim().to_ascii_lowercase().as_str() {
        "allow" => Some(true),
        "deny" => Some(false),
        _ => None,
    }
}

fn split_set(t: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in t.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if !out.iter().any(|o| o == part) {
            out.push(part.to_owned());
        }
    }
    out
}

/// `zombie` -> `minecraft:zombie`, lower case.
pub fn entity_id(s: &str) -> String {
    let s = s.trim().to_ascii_lowercase();
    if s.contains(':') { s } else { format!("minecraft:{s}") }
}

/// Whether an entity type in a set matches `kind` (`minecraft:zombie`), with or without the
/// namespace, case-insensitively.
pub fn entity_matches(entry: &str, kind: &str) -> bool {
    let strip = |s: &str| -> String {
        let s = s.trim().to_ascii_lowercase();
        s.strip_prefix("minecraft:").map(str::to_owned).unwrap_or(s)
    };
    strip(entry) == strip(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_the_documented_flags() {
        assert!(FLAGS.len() >= 99);
        assert!(lookup("BUILD").unwrap().protection);
        assert!(lookup("pvp").unwrap().protection);
        assert_eq!(lookup("entry").unwrap().default_group, Group::NonMembers);
        assert_eq!(lookup("blockbreak").unwrap().name, "block-break");
        assert!(lookup_exact("blockbreak").is_none());
        assert_eq!(lookup("exit-via-teleport").unwrap().default_allow, Some(true));
        let mut names: Vec<_> = FLAGS.iter().map(|f| f.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), FLAGS.len());
    }

    #[test]
    fn values_round_trip() {
        let me = Location { world: "world".into(), pos: [1.0, 2.0, 3.0], yaw: 4.0, pitch: 5.0 };
        let g = lookup("greeting").unwrap();
        let v = FlagValue::parse_input(g, "&aHi\\nthere", Some(&me)).unwrap().unwrap();
        assert_eq!(v, FlagValue::Str("\u{a7}aHi\nthere".into()));
        assert_eq!(FlagValue::from_yaml(Some(g), &v.to_yaml()), v);
        let d = lookup("deny-spawn").unwrap();
        let v = FlagValue::parse_input(d, "zombie, Creeper,zombie", Some(&me)).unwrap().unwrap();
        assert_eq!(
            v,
            FlagValue::Set(vec!["minecraft:zombie".into(), "minecraft:creeper".into()])
        );
        assert!(entity_matches("zombie", "minecraft:zombie"));
        let c = lookup("blocked-cmds").unwrap();
        let v = FlagValue::parse_input(c, "Home, /Spawn", None).unwrap().unwrap();
        assert_eq!(v, FlagValue::Set(vec!["/home".into(), "/spawn".into()]));
        let s = lookup("pvp").unwrap();
        assert_eq!(FlagValue::from_yaml(Some(s), &Value::Str("deny".into())), FlagValue::State(false));
        assert!(FlagValue::parse_input(s, "maybe", None).is_err());
        assert_eq!(FlagValue::parse_input(s, "NONE", None), Ok(None));
        assert_eq!(
            FlagValue::parse_input(s, "true", None).unwrap_err(),
            "Expected none/allow/deny but got 'true'"
        );
        let l = lookup("teleport").unwrap();
        let v = FlagValue::parse_input(l, "1,2,3,90,10", Some(&me)).unwrap().unwrap();
        assert_eq!(v, FlagValue::Location(Location { world: "world".into(), pos: [1.0, 2.0, 3.0], yaw: 90.0, pitch: 10.0 }));
        assert!(FlagValue::parse_input(l, "1 2 3", Some(&me)).is_err());
        assert_eq!(Group::parse("nonmembers"), Some(Group::NonMembers));
        assert_eq!(Group::parse("non_members"), Some(Group::NonMembers));
        assert_eq!(Group::parse("noone"), Some(Group::None));
        assert_eq!(Group::parse("NON_MEMBERS").unwrap().file_name(), "NON_MEMBERS");
        let raw = Value::Map(vec![("a".into(), Value::Int(1))]);
        assert_eq!(FlagValue::from_yaml(None, &raw), FlagValue::Raw(raw));
    }
}
