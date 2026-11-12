//! `config/worldguard-common.toml` (keys named like WorldGuard's `config.yml` and per-world
//! `config.yml`, in the same sections, with WorldGuard's defaults).

use std::collections::BTreeMap;

pub const DEFAULT_TOML: &str = r#"# horizon-worldguard settings. Keys and sections follow WorldGuard's config.yml (and the per-world
# config.yml, merged into this one file) where one exists; a key left out has WorldGuard's default.

# Operators of this level and above have every WorldGuard permission (WorldGuard's op-permissions).
op-permission-level = 2
# Where region files live: <data-folder>/worlds/<world>/regions.yml, relative to the server
# directory. The default is the Paper plugin's folder, so both can read the same files.
data-folder = "plugins/WorldGuard"
# Seconds after a change before the region files are written (they are also written on stop).
save-delay-seconds = 5
# /rg subcommands non-operators may run (Horizon has no permission nodes yet). Changes are then
# limited to regions they own. Empty, as WorldGuard without a permissions plugin.
player-commands = []

# Greeting/farewell/entry/exit and the movement flags need this.
use-player-move-event = true
# Entry/exit and teleport flags are checked for teleports too.
use-player-teleports = true

# The world folder name of each dimension.
[worlds]
"minecraft:overworld" = "world"
"minecraft:the_nether" = "world_nether"
"minecraft:the_end" = "world_the_end"

[regions]
enable = true
# true: operators must run /rg bypass to bypass protection; false: they bypass from the start.
disable-bypass-by-default = false
# Tell operators on join whether they bypass.
announce-bypass-status = false
# Check the mob-spawning and deny-spawn flags when a mob spawns.
use-creature-spawn-event = true
invincibility-removes-mobs = false
cancel-chat-without-recipients = true
nether-portal-protection = true
titles-always-use-default-times = true
fake-player-build-override = true
# Explosion flags also protect item frames, paintings and armor stands: one blocked block
# cancels the blast.
explosion-flags-block-entity-damage = true
# fire-spread, water-flow, lava-flow and lava-fire are only checked with this on.
high-frequency-flags = false
protect-against-liquid-flow = false
# The item that shows the regions at a block on right click (operators).
wand = "minecraft:leather"
max-claim-volume = 30000
claim-only-inside-existing-regions = false
set-parent-on-claim = ""
location-flags-only-inside-regions = false
# Pistons may not move blocks into regions the piston is not a member of (this port's own key;
# WorldGuard does this through its build query on the piston).
piston-border-protection = true

# How many regions a player may own: `default`, and a number per permission group.
[regions.max-region-count-per-player]
default = 7

[protection]
use-max-priority-association = false

# Overrides of the deny messages. WorldGuard has no such keys: the messages are the defaults of the
# deny-message, entry-deny-message and exit-deny-message flags, which a region's flag replaces.
[messages]
deny = "&c&lHey!&7 Sorry, but you can't %what% here."
entry-deny = "&c&lHey!&7 You are not permitted to enter this area."
exit-deny = "&c&lHey!&7 You are not permitted to leave this area."

# World options (WorldGuard's per-world config.yml sections). Only the ones below are acted on:
#   mobs.block-*-explosions / -block-damage, ignition.block-tnt / -block-damage,
#   fire.disable-all-fire-spread, fire.disable-lava-fire-spread, dynamics.disable-* (ice, snow,
#   leaf decay, growth, soil, coral, copper), player-damage.disable-fall-damage / -mob-damage /
#   -explosion-damage.
# All default to false, except mobs.allow-tamed-spawns and mobs.block-plugin-spawning (true).
[mobs]
[ignition]
[fire]
[dynamics]
[player-damage]
"#;

/// The sections of WorldGuard's per-world `config.yml` whose boolean keys are read.
const RULE_SECTIONS: [&str; 15] = [
    "mobs",
    "ignition",
    "fire",
    "dynamics",
    "player-damage",
    "physics",
    "weather",
    "crops",
    "turtle-egg",
    "sniffer-egg",
    "gameplay",
    "simulation",
    "default",
    "chest-protection",
    "event-handling",
];

/// Rule keys that are true unless the file says otherwise.
const RULE_DEFAULT_TRUE: [&str; 4] = [
    "mobs.allow-tamed-spawns",
    "mobs.block-plugin-spawning",
    "chest-protection.disable-off-check",
    "event-handling.break-hoppers-on-denied-move",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub op_level: u8,
    pub disable_bypass_by_default: bool,
    pub announce_bypass_status: bool,
    pub player_commands: Vec<String>,
    pub data_folder: String,
    pub save_delay_seconds: u32,
    pub worlds: BTreeMap<String, String>,
    pub enable: bool,
    pub wand: String,
    pub max_claim_volume: i64,
    pub claim_only_inside_existing: bool,
    pub set_parent_on_claim: String,
    /// `regions.max-region-count-per-player.default`.
    pub max_region_count: i64,
    /// The other keys of `regions.max-region-count-per-player`: a limit per permission group.
    pub max_region_counts: BTreeMap<String, i64>,
    pub use_player_move_event: bool,
    pub use_player_teleports: bool,
    pub use_creature_spawn_event: bool,
    pub invincibility_removes_mobs: bool,
    pub cancel_empty_chat: bool,
    pub titles_default_times: bool,
    pub use_max_priority_association: bool,
    pub high_frequency_flags: bool,
    pub protect_against_liquid_flow: bool,
    pub explosion_flags_block_entity_damage: bool,
    pub piston_border_protection: bool,
    pub nether_portal_protection: bool,
    pub fake_player_build_override: bool,
    pub location_flags_only_inside_regions: bool,
    pub deny_message: String,
    pub entry_deny_message: String,
    pub exit_deny_message: String,
    /// `section.key` -> value of the world options in `RULE_SECTIONS`.
    pub rules: BTreeMap<String, bool>,
}

impl Default for Config {
    fn default() -> Self {
        Config::parse(DEFAULT_TOML).expect("the default config parses")
    }
}

impl Config {
    /// A world option (`mobs.block-creeper-explosions`), with WorldGuard's default.
    pub fn rule(&self, key: &str) -> bool {
        self.rules.get(key).copied().unwrap_or_else(|| RULE_DEFAULT_TRUE.contains(&key))
    }

    /// `getMaxRegionCount(player)`: the largest limit among the player's groups, else the default.
    pub fn max_regions_for(&self, groups: &[String]) -> i64 {
        let mut best: Option<i64> = None;
        for g in groups {
            if let Some(n) = self.max_region_counts.get(&g.to_ascii_lowercase()) {
                best = Some(best.map_or(*n, |b| b.max(*n)));
            }
        }
        best.unwrap_or(self.max_region_count)
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let t: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let empty = toml::Table::new();
        let regions = t.get("regions").and_then(|v| v.as_table()).unwrap_or(&empty);
        let protection = t.get("protection").and_then(|v| v.as_table()).unwrap_or(&empty);
        let messages = t.get("messages").and_then(|v| v.as_table()).unwrap_or(&empty);
        let int = |tab: &toml::Table, k: &str, d: i64| tab.get(k).and_then(|v| v.as_integer()).unwrap_or(d);
        let boolean = |tab: &toml::Table, k: &str, d: bool| tab.get(k).and_then(|v| v.as_bool()).unwrap_or(d);
        let string = |tab: &toml::Table, k: &str, d: &str| {
            tab.get(k)
                .and_then(|v| v.as_str())
                .unwrap_or(d)
                .to_owned()
        };
        // Older files had these at another level: `[regions]` for the move event, the top level
        // for the bypass keys.
        let regions_or_top = |k: &str, d: bool| boolean(regions, k, boolean(&t, k, d));
        let mut worlds = BTreeMap::new();
        if let Some(w) = t.get("worlds").and_then(|v| v.as_table()) {
            for (dim, name) in w {
                if let Some(n) = name.as_str() {
                    worlds.insert(dim.clone(), n.to_owned());
                }
            }
        }
        if worlds.is_empty() {
            worlds.insert("minecraft:overworld".into(), "world".into());
            worlds.insert("minecraft:the_nether".into(), "world_nether".into());
            worlds.insert("minecraft:the_end".into(), "world_the_end".into());
        }
        let mut max_region_count = 7i64;
        let mut max_region_counts = BTreeMap::new();
        match regions.get("max-region-count-per-player") {
            Some(toml::Value::Integer(n)) => max_region_count = *n,
            Some(toml::Value::Table(tab)) => {
                for (k, v) in tab {
                    if let Some(n) = v.as_integer() {
                        if k == "default" {
                            max_region_count = n;
                        } else {
                            max_region_counts.insert(k.to_ascii_lowercase(), n);
                        }
                    }
                }
            }
            _ => {}
        }
        let mut rules = BTreeMap::new();
        for section in RULE_SECTIONS {
            if let Some(tab) = t.get(section).and_then(|v| v.as_table()) {
                for (k, v) in tab {
                    if let Some(b) = v.as_bool() {
                        rules.insert(format!("{section}.{k}"), b);
                    }
                }
            }
        }
        let cfg = Config {
            op_level: int(&t, "op-permission-level", 2).clamp(0, 4) as u8,
            disable_bypass_by_default: regions_or_top("disable-bypass-by-default", false),
            announce_bypass_status: regions_or_top("announce-bypass-status", false),
            player_commands: t
                .get("player-commands")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_ascii_lowercase))
                        .collect()
                })
                .unwrap_or_default(),
            data_folder: string(&t, "data-folder", "plugins/WorldGuard"),
            save_delay_seconds: int(&t, "save-delay-seconds", 5).clamp(0, 3600) as u32,
            worlds,
            enable: boolean(regions, "enable", true),
            wand: string(regions, "wand", "minecraft:leather"),
            max_claim_volume: int(regions, "max-claim-volume", 30000),
            claim_only_inside_existing: boolean(regions, "claim-only-inside-existing-regions", false),
            set_parent_on_claim: string(regions, "set-parent-on-claim", ""),
            max_region_count,
            max_region_counts,
            use_player_move_event: regions_or_top("use-player-move-event", true),
            use_player_teleports: regions_or_top("use-player-teleports", true),
            use_creature_spawn_event: boolean(regions, "use-creature-spawn-event", true),
            invincibility_removes_mobs: boolean(regions, "invincibility-removes-mobs", false),
            cancel_empty_chat: boolean(regions, "cancel-chat-without-recipients", true),
            titles_default_times: boolean(regions, "titles-always-use-default-times", true),
            use_max_priority_association: boolean(protection, "use-max-priority-association", false),
            high_frequency_flags: boolean(regions, "high-frequency-flags", false),
            protect_against_liquid_flow: boolean(regions, "protect-against-liquid-flow", false),
            explosion_flags_block_entity_damage: boolean(
                regions,
                "explosion-flags-block-entity-damage",
                true,
            ),
            piston_border_protection: boolean(regions, "piston-border-protection", true),
            nether_portal_protection: boolean(regions, "nether-portal-protection", true),
            fake_player_build_override: boolean(regions, "fake-player-build-override", true),
            location_flags_only_inside_regions: boolean(regions, "location-flags-only-inside-regions", false),
            deny_message: string(
                messages,
                "deny",
                super::flags::DEFAULT_DENY_MESSAGE,
            ),
            entry_deny_message: string(
                messages,
                "entry-deny",
                super::flags::DEFAULT_ENTRY_DENY_MESSAGE,
            ),
            exit_deny_message: string(
                messages,
                "exit-deny",
                super::flags::DEFAULT_EXIT_DENY_MESSAGE,
            ),
            rules,
        };
        Ok(cfg)
    }

    /// The world folder name for a dimension id; unknown dimensions use their path
    /// (`mymod:moon` -> `moon`).
    pub fn world_of(&self, dimension: &str) -> String {
        self.worlds.get(dimension).cloned().unwrap_or_else(|| {
            dimension
                .rsplit(':')
                .next()
                .unwrap_or(dimension)
                .to_owned()
        })
    }

    pub fn dimension_of(&self, world: &str) -> Option<String> {
        self.worlds
            .iter()
            .find(|(_, w)| w.eq_ignore_ascii_case(world))
            .map(|(d, _)| d.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_servers() {
        let c = Config::default();
        assert_eq!(c.op_level, 2);
        assert_eq!(c.wand, "minecraft:leather");
        assert_eq!(c.max_claim_volume, 30000);
        assert_eq!(c.max_region_count, 7);
        assert_eq!(c.max_regions_for(&["vip".into()]), 7);
        assert!(c.use_player_move_event);
        assert!(c.use_creature_spawn_event);
        assert!(c.rule("mobs.allow-tamed-spawns"));
        assert!(!c.rule("mobs.block-creeper-explosions"));
        assert!(!c.high_frequency_flags);
        assert_eq!(c.world_of("minecraft:the_nether"), "world_nether");
        assert_eq!(c.dimension_of("WORLD").as_deref(), Some("minecraft:overworld"));
        assert_eq!(c.world_of("mymod:moon"), "moon");
    }

    #[test]
    fn partial_files_fill_in() {
        let c = Config::parse("op-permission-level = 4\n[regions]\nwand = \"minecraft:stick\"\n").unwrap();
        assert_eq!(c.op_level, 4);
        assert_eq!(c.wand, "minecraft:stick");
        assert_eq!(c.max_region_count, 7);
        assert!(Config::parse("op-permission-level = ").is_err());
    }
}
