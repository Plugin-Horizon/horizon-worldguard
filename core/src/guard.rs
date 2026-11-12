//! Protection decisions for game events: one function per kind of action, each returning
//! whether it may happen. The trait mod calls these from its hooks.

use super::config::Config;
use super::flags::FlagValue;
use super::geom::BlockPos;
use super::kinds::{self, ItemUse};
use super::query::{Actor, Query, St};
use super::text;
use super::region::Subject;
use super::store::World;
use std::collections::{BTreeMap, HashSet};

/// A player as protection needs them.
#[derive(Clone, Debug)]
pub struct Player {
    pub subject: Subject,
    pub op_level: u8,
    pub dimension: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub game_mode: i32,
}

impl Player {
    pub fn block(&self) -> BlockPos {
        BlockPos::of(self.pos)
    }
}

/// A denied action: `what` fills `%what%` of the deny message.
#[derive(Clone, Debug, PartialEq)]
pub struct Denied {
    pub what: &'static str,
    /// The text to send, colour codes included (the region's `deny-message` or the config's).
    pub message: String,
}

pub type Outcome = Result<(), Denied>;

/// What blew up, as far as it is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blast {
    Tnt,
    Creeper,
    /// Ghast fireballs and other fireballs (`ghast-fireball`).
    Fireball,
    /// Wither skulls: a fireball for the flag (`ghast-fireball`), with their own options.
    WitherSkull,
    Wither,
    EnderDragon,
    /// A breeze's wind charge (`breeze-charge-explosion`).
    BreezeCharge,
    /// A player's wind charge: no explosion flag.
    WindCharge,
    Other,
}

impl Blast {
    pub fn from_kind(kind: Option<&str>) -> Blast {
        let Some(k) = kind else { return Blast::Other };
        match k.strip_prefix("minecraft:").unwrap_or(k) {
            "tnt" | "tnt_minecart" => Blast::Tnt,
            "creeper" => Blast::Creeper,
            "fireball" | "small_fireball" | "dragon_fireball" => Blast::Fireball,
            "wither_skull" => Blast::WitherSkull,
            "wither" => Blast::Wither,
            "ender_dragon" => Blast::EnderDragon,
            "breeze_wind_charge" => Blast::BreezeCharge,
            "wind_charge" => Blast::WindCharge,
            _ => Blast::Other,
        }
    }
    fn flag(self) -> Option<&'static str> {
        Some(match self {
            Blast::Tnt => "tnt",
            Blast::Creeper => "creeper-explosion",
            Blast::Fireball | Blast::WitherSkull => "ghast-fireball",
            Blast::Wither => "wither-damage",
            Blast::EnderDragon => "enderdragon-block-damage",
            Blast::BreezeCharge => "breeze-charge-explosion",
            Blast::WindCharge => return None,
            Blast::Other => "other-explosion",
        })
    }
    /// The world options that cancel the blast, and those that only spare the blocks.
    fn rules(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Blast::Creeper => (&["mobs.block-creeper-explosions"], &["mobs.block-creeper-block-damage"]),
            Blast::EnderDragon => (&[], &["mobs.block-enderdragon-block-damage"]),
            Blast::Tnt => (&["ignition.block-tnt"], &["ignition.block-tnt-block-damage"]),
            Blast::WitherSkull => (
                &["mobs.block-wither-skull-explosions"],
                &["mobs.block-wither-skull-block-damage"],
            ),
            Blast::Fireball => (&["mobs.block-fireball-explosions"], &["mobs.block-fireball-block-damage"]),
            Blast::BreezeCharge | Blast::WindCharge => (&["mobs.block-windcharge-explosions"], &[]),
            Blast::Wither => (&["mobs.block-wither-explosions"], &["mobs.block-wither-block-damage"]),
            Blast::Other => (&["mobs.block-other-explosions"], &[]),
        }
    }
}

#[derive(Default)]
pub struct Guard {
    pub cfg: Config,
    /// World folder name -> its regions.
    pub worlds: BTreeMap<String, World>,
    /// Operators who switched bypass off (or on, with `disable-bypass-by-default`).
    pub bypass_toggled: HashSet<u128>,
}

impl Guard {
    pub fn new(cfg: Config) -> Guard {
        Guard {
            cfg,
            ..Guard::default()
        }
    }

    pub fn world(&self, dimension: &str) -> Option<&World> {
        self.worlds.get(&self.cfg.world_of(dimension))
    }

    pub fn world_mut(&mut self, dimension: &str) -> &mut World {
        let name = self.cfg.world_of(dimension);
        self.worlds
            .entry(name.clone())
            .or_insert_with(|| World::new(&name))
    }

    pub fn is_admin(&self, p: &Player) -> bool {
        p.op_level >= self.cfg.op_level
    }

    pub fn bypasses(&self, p: &Player) -> bool {
        if !self.cfg.enable {
            return false;
        }
        if self.cfg.fake_player_build_override && is_fake_player(&p.subject.name) {
            return true;
        }
        if !self.is_admin(p) {
            return false;
        }
        let toggled = self.bypass_toggled.contains(&p.subject.uuid);
        toggled == self.cfg.disable_bypass_by_default
    }

    /// Switches bypass for an operator; returns whether they bypass now.
    pub fn toggle_bypass(&mut self, p: &Player) -> bool {
        let id = p.subject.uuid;
        if !self.bypass_toggled.remove(&id) {
            self.bypass_toggled.insert(id);
        }
        self.bypasses(p)
    }

    /// Sets bypass for an operator (`/rg bypass on|off`); returns whether they bypass now.
    pub fn set_bypass(&mut self, p: &Player, on: bool) -> bool {
        let id = p.subject.uuid;
        // bypasses() is `toggled == disable-bypass-by-default`, so toggled must equal (on == default).
        if on == self.cfg.disable_bypass_by_default {
            self.bypass_toggled.insert(id);
        } else {
            self.bypass_toggled.remove(&id);
        }
        self.bypasses(p)
    }

    /// The text of a deny message: the region's `deny-message` (or the default) with the macros of
    /// `replaceMacros` that need no server state, the colour macros, and `%what%`. `%online%` and
    /// `%health%` are left for the caller.
    fn deny_text(&self, w: Option<&World>, at: BlockPos, p: Option<&Player>, what: &str) -> String {
        let custom = w.and_then(|w| {
            let actor = p.map_or(Actor::Nobody, |p| Actor::Player(&p.subject));
            Query::new(w)
                .value(at, "deny-message", &actor)
                .and_then(|v| v.as_str().map(str::to_owned))
        });
        let template = custom.unwrap_or_else(|| self.cfg.deny_message.clone());
        let mut m = template;
        if let Some(p) = p {
            m = m
                .replace("%name%", &p.subject.name)
                .replace("%id%", &super::region::uuid_string(p.subject.uuid))
                .replace("%world%", &self.cfg.world_of(&p.dimension));
        }
        text::color_macros(&m).replace("%what%", what)
    }

    fn deny(&self, w: Option<&World>, at: BlockPos, p: Option<&Player>, what: &'static str) -> Denied {
        Denied {
            what,
            message: self.deny_text(w, at, p, what),
        }
    }

    /// Build-type check for a player at `at` in their dimension (`testBuild`).
    fn build(&self, p: &Player, at: BlockPos, flags: &[&str], what: &'static str) -> Outcome {
        if !self.cfg.enable || self.bypasses(p) {
            return Ok(());
        }
        let Some(w) = self.world(&p.dimension) else { return Ok(()) };
        if Query::new(w).can_build(at, &Actor::Player(&p.subject), flags).allowed() {
            Ok(())
        } else {
            Err(self.deny(Some(w), at, Some(p), what))
        }
    }

    /// A state flag for a player that is not a build check (`testState`), without the bypass
    /// (chat is not bypassed).
    fn state_plain(&self, p: &Player, at: BlockPos, flag: &str, what: &'static str) -> Outcome {
        if !self.cfg.enable {
            return Ok(());
        }
        let Some(w) = self.world(&p.dimension) else { return Ok(()) };
        if Query::new(w).test_state(at, &[flag], &Actor::Player(&p.subject)) {
            Ok(())
        } else {
            Err(self.deny(Some(w), at, Some(p), what))
        }
    }

    /// A flag with no player behind it (natural events, mobs, blasts): `testState` with no
    /// subject; flags without a default (`tnt`) count as allowed unless a region denies them.
    fn natural(&self, dimension: &str, at: BlockPos, flag: &str) -> bool {
        if !self.cfg.enable {
            return true;
        }
        match self.world(dimension) {
            Some(w) => Query::new(w).allows(at, flag, &Actor::Nobody),
            None => true,
        }
    }

    // -- blocks ----------------------------------------------------------------------------

    pub fn break_block(&self, p: &Player, at: BlockPos) -> Outcome {
        self.build(p, at, &["block-break"], "break that block")
    }

    pub fn place_block(&self, p: &Player, at: BlockPos) -> Outcome {
        self.build(p, at, &["block-place"], "place that block")
    }

    /// A right click on `block` at `at` (before the block reacts): `onUseBlock`. Blocks that do
    /// nothing when clicked are let through (the held item is `use_item_on`'s concern).
    pub fn use_block(&self, p: &Player, at: BlockPos, block: &str) -> Outcome {
        if !kinds::modified_on_click(block, true) {
            return Ok(());
        }
        let (flags, what) = kinds::block_use_flags(block);
        self.build(p, at, flags, what)
    }

    /// Using `item` on the block `block` at `at` (clicked face `face`): `handleBlockRightClick`
    /// and the item-applied-to-block rules. A block item is checked by `place_block` when it places.
    pub fn use_item_on(&self, p: &Player, at: BlockPos, face: [i32; 3], item: &str, block: &str) -> Outcome {
        let target = BlockPos::new(at.x + face[0], at.y + face[1], at.z + face[2]);
        match kinds::item_use(item) {
            ItemUse::Tnt => {
                self.build(p, at, &["interact", "tnt"], "use explosives")?;
                return self.build(p, target, &["block-place"], "place that block");
            }
            ItemUse::Vehicle => {
                let spot = if kinds::path_of(item).ends_with("minecart") { at } else { target };
                return self.build(p, spot, &["vehicle-place"], "place vehicles");
            }
            ItemUse::Spawns => return self.build(p, target, &[], "place things"),
            ItemUse::Bucket | ItemUse::Lighter => return Ok(()),
            ItemUse::Other => {}
        }
        if kinds::item_applied_to_block(item, block) {
            let (flags, what) = kinds::block_use_flags(block);
            self.build(p, at, flags, what)?;
            return self.build(p, at, &["block-place"], "place that block");
        }
        Ok(())
    }

    /// `PlayerBucketEmptyEvent`: a place at the affected block.
    pub fn bucket(&self, p: &Player, at: BlockPos) -> Outcome {
        self.build(p, at, &["block-place"], "place that block")
    }

    /// `PlayerBucketFillEvent`: a break at the affected block.
    pub fn bucket_fill(&self, p: &Player, at: BlockPos) -> Outcome {
        self.build(p, at, &["block-break"], "break that block")
    }

    /// A frame or painting placed: a plain spawn.
    pub fn hanging_place(&self, p: &Player, at: BlockPos) -> Outcome {
        self.build(p, at, &[], "place things")
    }

    /// A frame or painting about to break: by a player, or by something else (blast, wall gone).
    pub fn hanging_break(&self, p: Option<&Player>, dimension: &str, at: BlockPos, kind: &str) -> Outcome {
        match p {
            Some(p) => self.build(p, at, &[], "break things"),
            None => {
                let flag = if kind.ends_with("painting") {
                    "entity-painting-destroy"
                } else {
                    "entity-item-frame-destroy"
                };
                if !self.cfg.enable {
                    return Ok(());
                }
                let ok = self.world(dimension).is_none_or(|w| {
                    Query::new(w).can_build(at, &Actor::Nobody, &["other-explosion", flag]).allowed()
                });
                if ok {
                    Ok(())
                } else {
                    Err(Denied { what: "break things", message: String::new() })
                }
            }
        }
    }

    // -- entities --------------------------------------------------------------------------

    /// A player's blow on an entity of `kind` at `at`: `onDamageEntity`. `victim` is set for
    /// another player.
    pub fn attack_entity(&self, p: &Player, kind: &str, at: [f64; 3], victim: Option<&Player>) -> Outcome {
        let at_b = BlockPos::of(at);
        if let Some(v) = victim {
            return self.pvp(p, v);
        }
        if kinds::is_hostile(kind) || kinds::is_ambient(kind) {
            return Ok(());
        }
        if kinds::is_vehicle(kind) {
            return self.build(p, at_b, &["vehicle-destroy"], "change that");
        }
        if kinds::is_hanging_or_stand(kind) {
            return self.build(p, at_b, &[], "change that");
        }
        if kinds::is_animal(kind) {
            return self.build(p, at_b, &["damage-animals"], "harm that");
        }
        self.build(p, at_b, &["interact"], "hit that")
    }

    /// PvP: `testBuild` at the victim for the attacker with `pvp`, and no `deny` of `pvp` at the
    /// attacker's or the victim's spot for the attacker. Bypass does not apply to PvP, only the
    /// fake player override does.
    pub fn pvp(&self, attacker: &Player, victim: &Player) -> Outcome {
        if !self.cfg.enable || (self.cfg.fake_player_build_override && is_fake_player(&attacker.subject.name)) {
            return Ok(());
        }
        let me = Actor::Player(&attacker.subject);
        let mut ok = true;
        if let Some(w) = self.world(&victim.dimension) {
            let q = Query::new(w);
            ok = q.can_build(victim.block(), &me, &["pvp"]).allowed()
                && q.query_state(victim.block(), &["pvp"], &me) != Some(St::Deny);
        }
        if ok
            && let Some(w) = self.world(&attacker.dimension)
        {
            ok = Query::new(w).query_state(attacker.block(), &["pvp"], &me) != Some(St::Deny);
        }
        if ok {
            Ok(())
        } else {
            let w = self.world(&victim.dimension);
            Err(self.deny(w, victim.block(), Some(attacker), "PvP"))
        }
    }

    /// A right click on an entity: `onUseEntity`.
    pub fn interact_entity(&self, p: &Player, kind: &str, at: [f64; 3]) -> Outcome {
        let at = BlockPos::of(at);
        let k = kind.strip_prefix("minecraft:").unwrap_or(kind);
        if kinds::is_hostile(kind) || kinds::is_ambient(kind) || k == "player" {
            return Ok(());
        }
        if k.ends_with("chest_boat") || k.ends_with("chest_raft") || k == "chest_minecart" || k == "hopper_minecart" {
            return self.build(p, at, &["chest-access"], "open that");
        }
        if kinds::is_hanging_or_stand(kind) {
            return self.build(p, at, &[], "change that");
        }
        if kinds::is_rideable(kind) {
            return Ok(());
        }
        self.build(p, at, &["interact"], "use that")
    }

    /// A rotation of an item frame that holds an item.
    pub fn rotate_frame(&self, p: &Player, at: [f64; 3]) -> Outcome {
        self.build(p, BlockPos::of(at), &["item-frame-rotation"], "change that")
    }

    /// `EntityMountEvent`: the vehicle's spot, `ride` and `interact`.
    pub fn mount(&self, p: &Player, vehicle_kind: &str, at: [f64; 3]) -> Outcome {
        let _ = vehicle_kind;
        self.build(p, BlockPos::of(at), &["ride", "interact"], "ride that")
    }

    /// Damage to a player at `at`. `attacker` is another player for PvP (projectiles included),
    /// `by_mob` for mob damage.
    pub fn damage_player(
        &self,
        victim: &Player,
        damage_type: &str,
        attacker: Option<&Player>,
        by_mob: bool,
    ) -> bool {
        if !self.cfg.enable {
            return true;
        }
        let Some(w) = self.world(&victim.dimension) else { return true };
        let q = Query::new(w);
        let at = victim.block();
        let me = Actor::Player(&victim.subject);
        if q.state(at, "invincible", &me) == Some(true) {
            return false;
        }
        let t = damage_type.strip_prefix("minecraft:").unwrap_or(damage_type);
        if (t == "fall" || t == "fly_into_wall") && !q.test_state(at, &["fall-damage"], &me) {
            return false;
        }
        if t == "fall" && self.cfg.rule("player-damage.disable-fall-damage") {
            return false;
        }
        if by_mob && self.cfg.rule("player-damage.disable-mob-damage") {
            return false;
        }
        if t == "firework" || t == "fireworks" {
            // Only an explicit deny stops it (the damage query is not a build query).
            let actor = attacker.map_or(Actor::Nobody, |a| Actor::Player(&a.subject));
            return q.query_state(at, &["firework-damage"], &actor) != Some(St::Deny);
        }
        if let Some(a) = attacker
            && a.subject.uuid != victim.subject.uuid
        {
            return self.pvp(a, victim).is_ok();
        }
        if by_mob && !q.test_state(at, &["mob-damage"], &me) {
            return false;
        }
        true
    }

    pub fn mob_spawn(&self, dimension: &str, kind: &str, at: [f64; 3]) -> bool {
        if !self.cfg.enable || !self.cfg.use_creature_spawn_event || !kinds::is_mob(kind) {
            return true;
        }
        let Some(w) = self.world(dimension) else { return true };
        let q = Query::new(w);
        let b = BlockPos::of(at);
        if !q.test_state(b, &["mob-spawning"], &Actor::Nobody) {
            return false;
        }
        if let Some(FlagValue::Set(deny)) = q.value(b, "deny-spawn", &Actor::Nobody)
            && deny.iter().any(|e| super::flags::entity_matches(e, kind))
        {
            return false;
        }
        true
    }

    // -- blasts, fire, nature --------------------------------------------------------------

    /// Removes from `blocks` those the blast may not break: the explosion flag of the source
    /// (`RegionFlagsListener`, `WorldGuardEntityListener`) and the break check of the source's
    /// regions (`RegionProtectionListener`).
    pub fn explosion(&self, dimension: &str, center: [f64; 3], blast: Blast, blocks: &mut Vec<[i32; 3]>) {
        if !self.cfg.enable {
            return;
        }
        let Some(w) = self.world(dimension) else { return };
        let q = Query::new(w);
        let source = Actor::Source(BlockPos::of(center));
        let (cancel_rules, spare_rules) = blast.rules();
        if cancel_rules.iter().chain(spare_rules).any(|r| self.cfg.rule(r)) {
            // The explosion happens without damaging a block (it cannot be stopped from here).
            blocks.clear();
            return;
        }
        if blast != Blast::Tnt
            && let Some(flag) = blast.flag()
        {
            let before = blocks.len();
            blocks.retain(|b| q.allows(BlockPos::from(*b), flag, &Actor::Nobody));
            let failed = blocks.len() != before;
            // Wither, fireballs and unknown sources drop the whole blast on one failure; the
            // others do when `explosion-flags-block-entity-damage` is on.
            let whole = matches!(blast, Blast::Fireball | Blast::WitherSkull | Blast::Wither | Blast::Other)
                || self.cfg.explosion_flags_block_entity_damage;
            if failed && whole {
                blocks.clear();
                return;
            }
        }
        let specific: &[&str] = if blast == Blast::Tnt { &["block-break", "tnt"] } else { &["block-break"] };
        blocks.retain(|b| q.can_build(BlockPos::from(*b), &source, specific).allowed());
    }

    /// TNT about to be primed (by a player's click when `p` is set).
    pub fn tnt_prime(&self, p: Option<&Player>, dimension: &str, at: BlockPos) -> Outcome {
        match p {
            Some(p) => self.build(p, at, &["tnt"], "use explosives"),
            None if self.natural(dimension, at, "tnt") => Ok(()),
            None => Err(Denied { what: "use explosives", message: String::new() }),
        }
    }

    pub fn fire_spread(&self, dimension: &str, to: BlockPos) -> bool {
        if self.cfg.rule("fire.disable-all-fire-spread") {
            return false;
        }
        !self.cfg.high_frequency_flags || self.natural(dimension, to, "fire-spread")
    }

    pub fn ignite(&self, p: Option<&Player>, dimension: &str, at: BlockPos, cause: &str) -> Outcome {
        if let Some(p) = p {
            return self.build(p, at, &["block-place", "lighter"], "place fire");
        }
        let c = cause.to_ascii_lowercase();
        let flag = if c.contains("lava") {
            if self.cfg.rule("fire.disable-lava-fire-spread") {
                return Err(Denied { what: "place fire", message: String::new() });
            }
            if !self.cfg.high_frequency_flags {
                return Ok(());
            }
            "lava-fire"
        } else if c.contains("lightning") {
            "lightning"
        } else {
            if !self.cfg.high_frequency_flags {
                return Ok(());
            }
            "fire-spread"
        };
        if self.natural(dimension, at, flag) {
            Ok(())
        } else {
            Err(Denied { what: "light fires", message: String::new() })
        }
    }

    pub fn burn(&self, dimension: &str, at: BlockPos) -> bool {
        if self.cfg.rule("fire.disable-all-fire-spread") {
            return false;
        }
        !self.cfg.high_frequency_flags || self.natural(dimension, at, "fire-spread")
    }

    pub fn leaf_decay(&self, dimension: &str, at: BlockPos) -> bool {
        !self.cfg.rule("dynamics.disable-leaf-decay") && self.natural(dimension, at, "leaf-decay")
    }

    /// A fluid flowing from `from` into `to`.
    pub fn fluid_flow(&self, dimension: &str, fluid: &str, from: BlockPos, to: BlockPos) -> bool {
        if !self.cfg.enable {
            return true;
        }
        if self.cfg.high_frequency_flags {
            let flag = if fluid.contains("lava") { "lava-flow" } else { "water-flow" };
            if !self.natural(dimension, to, flag) {
                return false;
            }
        }
        if self.cfg.protect_against_liquid_flow
            && let Some(w) = self.world(dimension)
        {
            return Query::new(w).can_build(to, &Actor::Source(from), &[]).allowed();
        }
        true
    }

    /// A block fading or melting (`block` is the block before).
    pub fn block_fade(&self, dimension: &str, at: BlockPos, block: &str) -> bool {
        let b = block.strip_prefix("minecraft:").unwrap_or(block);
        let (flag, rule) = match b {
            "ice" => ("ice-melt", "dynamics.disable-ice-melting"),
            "frosted_ice" => ("frosted-ice-melt", ""),
            "snow" | "snow_block" => ("snow-melt", "dynamics.disable-snow-melting"),
            "farmland" => ("soil-dry", "dynamics.disable-soil-dehydration"),
            _ if b.contains("coral") => ("coral-fade", "dynamics.disable-coral-block-fade"),
            _ if b.contains("copper") => ("copper-fade", "dynamics.disable-copper-block-fade"),
            _ => return true,
        };
        !self.cfg.rule(rule) && self.natural(dimension, at, flag)
    }

    /// A block forming (`block` is the new block).
    pub fn block_form(&self, dimension: &str, at: BlockPos, block: &str) -> bool {
        let b = block.strip_prefix("minecraft:").unwrap_or(block);
        let (flag, rule) = match b {
            "ice" => ("ice-form", "dynamics.disable-ice-formation"),
            "snow" => ("snow-fall", "dynamics.disable-snow-formation"),
            "frosted_ice" => ("frosted-ice-form", ""),
            _ => return true,
        };
        !self.cfg.rule(rule) && self.natural(dimension, at, flag)
    }

    /// A block spreading or growing into `at` (`block` is the new block).
    pub fn block_grow(&self, dimension: &str, at: BlockPos, block: &str) -> bool {
        let b = block.strip_prefix("minecraft:").unwrap_or(block);
        let (flag, rule) = if b.contains("mushroom") {
            ("mushroom-growth", "dynamics.disable-mushroom-spread")
        } else if b == "grass_block" {
            ("grass-growth", "dynamics.disable-grass-growth")
        } else if b == "mycelium" {
            ("mycelium-spread", "dynamics.disable-mycelium-spread")
        } else if b.contains("vine") || b.contains("kelp") {
            ("vine-growth", "dynamics.disable-vine-growth")
        } else if b.contains("dripstone") {
            ("rock-growth", "dynamics.disable-rock-growth")
        } else if b.starts_with("sculk") {
            ("sculk-growth", "dynamics.disable-sculk-growth")
        } else {
            return true;
        };
        !self.cfg.rule(rule) && self.natural(dimension, at, flag)
    }

    /// A piston at `piston` moving `push` one step `dir` and destroying `destroy`.
    pub fn piston(&self, dimension: &str, piston: BlockPos, dir: [i32; 3], push: &[BlockPos], destroy: &[BlockPos]) -> bool {
        if !self.cfg.enable {
            return true;
        }
        let Some(w) = self.world(dimension) else { return true };
        let q = Query::new(w);
        let src = Actor::Source(piston);
        let moved = push.iter().map(|b| BlockPos::new(b.x + dir[0], b.y + dir[1], b.z + dir[2]));
        let touched: Vec<BlockPos> = push.iter().copied().chain(moved).chain(destroy.iter().copied()).collect();
        for b in touched {
            if !q.allows(b, "pistons", &Actor::Nobody) {
                return false;
            }
            if self.cfg.piston_border_protection && !q.can_build(b, &src, &["block-break"]).allowed() {
                return false;
            }
        }
        true
    }

    // -- players ---------------------------------------------------------------------------

    pub fn item_pickup(&self, p: &Player) -> Outcome {
        self.build(p, p.block(), &["item-pickup"], "pick up items")
    }

    pub fn item_drop(&self, p: &Player) -> Outcome {
        self.build(p, p.block(), &["item-drop"], "drop items")
    }

    /// Ender pearls and chorus fruit: the flag at both ends; the player is told the exit or
    /// entry deny message of the end that refused.
    fn teleport_flag(&self, p: &Player, target: [f64; 3], flag: &str) -> Outcome {
        if !self.cfg.enable || self.bypasses(p) {
            return Ok(());
        }
        let me = Actor::Player(&p.subject);
        let ends = [(p.block(), &p.dimension, "exit-deny-message", &self.cfg.exit_deny_message),
            (BlockPos::of(target), &p.dimension, "entry-deny-message", &self.cfg.entry_deny_message)];
        for (at, dim, message_flag, default) in ends {
            let Some(w) = self.world(dim) else { continue };
            let q = Query::new(w);
            if !q.test_state(at, &[flag], &me) {
                let message = q
                    .value(at, message_flag, &me)
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| default.clone());
                return Err(Denied { what: "teleport", message });
            }
        }
        Ok(())
    }

    pub fn ender_pearl(&self, p: &Player, target: [f64; 3]) -> Outcome {
        self.teleport_flag(p, target, "enderpearl")
    }

    pub fn chorus(&self, p: &Player, target: [f64; 3]) -> Outcome {
        self.teleport_flag(p, target, "chorus-fruit-teleport")
    }

    pub fn sleep(&self, p: &Player, bed: BlockPos) -> Outcome {
        self.build(p, bed, &["interact", "sleep"], "sleep")
    }

    pub fn send_chat(&self, p: &Player) -> Outcome {
        self.state_plain(p, p.block(), "send-chat", "chat")
    }

    /// Whether `p` may receive chat where they stand.
    pub fn receive_chat(&self, p: &Player) -> bool {
        self.state_plain(p, p.block(), "receive-chat", "").is_ok()
    }

    /// `blocked-cmds` / `allowed-cmds` for a command line (`CommandFilter`).
    pub fn command(&self, p: &Player, line: &str) -> Outcome {
        if !self.cfg.enable || self.bypasses(p) {
            return Ok(());
        }
        let Some(w) = self.world(&p.dimension) else { return Ok(()) };
        let q = Query::new(w);
        let actor = Actor::Player(&p.subject);
        let at = p.block();
        let allowed = match q.value(at, "allowed-cmds", &actor) {
            Some(FlagValue::Set(s)) => Some(s),
            _ => None,
        };
        let blocked = match q.value(at, "blocked-cmds", &actor) {
            Some(FlagValue::Set(s)) => Some(s),
            _ => None,
        };
        if command_filter(allowed.as_deref(), blocked.as_deref(), line) {
            return Ok(());
        }
        let shown = format!("use /{}", line.trim_start_matches('/'));
        Err(Denied {
            what: "use that command",
            message: self.deny_text(Some(w), at, Some(p), &shown),
        })
    }
}

/// WorldGuard's `CommandFilter.apply`: whether `command` may run. A rule matches when its words
/// are the first words of the command; a matching `permitted` rule beats a matching `denied` one,
/// and when `permitted` has rules the command must match one of them.
pub fn command_filter(permitted: Option<&[String]>, denied: Option<&[String]>, command: &str) -> bool {
    let lower = command.to_lowercase();
    let t = lower.trim_start();
    let t = if t.starts_with('/') { t.to_owned() } else { format!("/{t}") };
    // "^/([^ :]*:)?" -> "/": drop a namespace like `minecraft:`.
    let after = &t[1..];
    let word_end = after.find(' ').unwrap_or(after.len());
    let normalized = match after[..word_end].find(':') {
        Some(i) => format!("/{}", &after[i + 1..]),
        None => t.clone(),
    };
    let used: Vec<&str> = normalized.split_whitespace().collect();
    let matches = |rule: &str| {
        let parts: Vec<&str> = rule.split_whitespace().collect();
        !parts.is_empty()
            && parts.len() <= used.len()
            && parts.iter().zip(&used).all(|(a, b)| a.eq_ignore_ascii_case(b))
    };
    let mut blocked = false;
    if let Some(d) = denied {
        blocked = d.iter().any(|r| matches(r));
    }
    if let Some(p) = permitted
        && !p.is_empty()
    {
        blocked = !p.iter().any(|r| matches(r));
    }
    !blocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::flags::FlagValue;
    use super::super::geom::Shape;
    use super::super::region::Region;

    fn player(n: u128, op: u8, pos: [f64; 3]) -> Player {
        Player {
            subject: Subject { uuid: n, name: format!("p{n}"), groups: vec![] },
            op_level: op,
            dimension: "minecraft:overworld".into(),
            pos,
            yaw: 0.0,
            pitch: 0.0,
            game_mode: 0,
        }
    }

    /// The archived lobby region shape: no members, five typical flags.
    fn lobby_guard() -> Guard {
        let mut g = Guard::new(Config::default());
        let w = g.world_mut("minecraft:overworld");
        let mut r = Region::new("lobby", Shape::cuboid(BlockPos::new(-20, 2, -20), BlockPos::new(20, 287, 20)));
        r.priority = 10;
        for f in ["block-break", "pvp", "mob-spawning", "block-place"] {
            r.set_flag(f, Some(FlagValue::State(false)));
        }
        w.put(r);
        w.global_mut().set_flag("tnt", Some(FlagValue::State(false)));
        g
    }

    #[test]
    fn lobby_rules() {
        let g = lobby_guard();
        let guest = player(1, 0, [0.5, 64.0, 0.5]);
        let other = player(2, 0, [1.5, 64.0, 0.5]);
        let op = player(3, 4, [0.5, 64.0, 0.5]);
        let inside = BlockPos::new(0, 63, 0);
        let outside = BlockPos::new(100, 63, 100);
        assert!(g.break_block(&guest, inside).is_err());
        assert!(g.place_block(&guest, inside).is_err());
        assert!(g.break_block(&guest, outside).is_ok());
        assert!(g.break_block(&op, inside).is_ok());
        assert_eq!(g.pvp(&guest, &other).unwrap_err().what, "PvP");
        assert!(!g.mob_spawn("minecraft:overworld", "minecraft:zombie", [0.0, 64.0, 0.0]));
        assert!(g.mob_spawn("minecraft:overworld", "minecraft:zombie", [100.0, 64.0, 0.0]));
        assert!(g.mob_spawn("minecraft:overworld", "minecraft:item", [0.0, 64.0, 0.0]));
        let mut blocks = vec![[100, 60, 100], [0, 60, 0]];
        g.explosion("minecraft:overworld", [100.0, 60.0, 100.0], Blast::Tnt, &mut blocks);
        assert!(blocks.is_empty(), "global tnt deny spares everything");
        let mut blocks = vec![[100, 60, 100]];
        g.explosion("minecraft:overworld", [100.0, 60.0, 100.0], Blast::Creeper, &mut blocks);
        assert_eq!(blocks.len(), 1);
        let msg = g.break_block(&guest, inside).unwrap_err().message;
        assert_eq!(msg, "\u{a7}c\u{a7}lHey!\u{a7}7 Sorry, but you can't break that block here.");
    }

    #[test]
    fn bypass_toggle() {
        let mut g = lobby_guard();
        let op = player(3, 4, [0.5, 64.0, 0.5]);
        assert!(g.bypasses(&op));
        assert!(!g.toggle_bypass(&op));
        assert!(g.break_block(&op, BlockPos::new(0, 63, 0)).is_err());
        assert!(g.toggle_bypass(&op));
    }

    #[test]
    fn commands_and_spawn_sets() {
        let mut g = Guard::new(Config::default());
        let w = g.world_mut("minecraft:overworld");
        let mut r = Region::new("jail", Shape::cuboid(BlockPos::new(0, 0, 0), BlockPos::new(10, 10, 10)));
        r.set_flag("blocked-cmds", Some(FlagValue::Set(vec!["/tp".into(), "/home".into()])));
        r.set_flag("deny-spawn", Some(FlagValue::Set(vec!["minecraft:creeper".into()])));
        w.put(r);
        let p = player(1, 0, [5.0, 5.0, 5.0]);
        assert!(g.command(&p, "tp someone").is_err());
        assert!(g.command(&p, "/minecraft:tp x").is_err());
        assert!(g.command(&p, "tpa x").is_ok());
        assert!(!g.mob_spawn("minecraft:overworld", "creeper", [1.0, 1.0, 1.0]));
        assert!(g.mob_spawn("minecraft:overworld", "zombie", [1.0, 1.0, 1.0]));
    }

    #[test]
    fn command_filter_rules() {
        let blocked = vec!["/tp".to_string(), "/warp spawn".to_string()];
        assert!(!command_filter(None, Some(&blocked), "/tp bob"));
        assert!(command_filter(None, Some(&blocked), "/warp"));
        assert!(!command_filter(None, Some(&blocked), "/warp spawn now"));
        assert!(command_filter(None, Some(&blocked), "/tpa bob"));
        let allowed = vec!["/spawn".to_string()];
        assert!(command_filter(Some(&allowed), None, "/spawn"));
        assert!(!command_filter(Some(&allowed), None, "/home"));
        // A matching permitted rule beats a matching denied one.
        assert!(command_filter(Some(&allowed), Some(&["/spawn".to_string()]), "/spawn"));
        assert!(command_filter(None, None, "/anything"));
    }

    #[test]
    fn pvp_in_a_protected_region_needs_the_flag() {
        let mut g = Guard::new(Config::default());
        let w = g.world_mut("minecraft:overworld");
        w.put(Region::new("plot", Shape::cuboid(BlockPos::new(0, 0, 0), BlockPos::new(10, 10, 10))));
        let a = player(1, 0, [20.0, 5.0, 5.0]);
        let b = player(2, 0, [5.0, 5.0, 5.0]);
        assert!(g.pvp(&a, &b).is_err(), "a non-member cannot hit into a protected region");
        g.world_mut("minecraft:overworld").get_mut("plot").unwrap().set_flag("pvp", Some(FlagValue::State(true)));
        assert!(g.pvp(&a, &b).is_ok());
    }

    #[test]
    fn item_drop_flag() {
        let mut g = lobby_guard();
        g.world_mut("minecraft:overworld")
            .get_mut("lobby")
            .unwrap()
            .set_flag("item-drop", Some(FlagValue::State(false)));
        let inside = player(1, 0, [0.5, 64.0, 0.5]);
        let outside = player(1, 0, [100.5, 64.0, 0.5]);
        let op = player(3, 4, [0.5, 64.0, 0.5]);
        let d = g.item_drop(&inside).unwrap_err();
        assert_eq!(d.message, "\u{a7}c\u{a7}lHey!\u{a7}7 Sorry, but you can't drop items here.");
        assert!(g.item_drop(&outside).is_ok());
        assert!(g.item_drop(&op).is_ok(), "bypass");
    }

    #[test]
    fn pistons_respect_borders() {
        let mut g = Guard::new(Config::default());
        let w = g.world_mut("minecraft:overworld");
        w.put(Region::new("plot", Shape::cuboid(BlockPos::new(0, 0, 0), BlockPos::new(10, 10, 10))));
        let d = "minecraft:overworld";
        // From outside, pushing into the plot.
        assert!(!g.piston(d, BlockPos::new(-2, 5, 5), [1, 0, 0], &[BlockPos::new(-1, 5, 5)], &[]));
        // Inside the plot.
        assert!(g.piston(d, BlockPos::new(2, 5, 5), [1, 0, 0], &[BlockPos::new(3, 5, 5)], &[]));
    }
}

/// Names like `[bot]`: the rule of `fake-player-build-override`.
fn is_fake_player(name: &str) -> bool {
    name.len() > 2 && name.starts_with('[') && name.ends_with(']')
}
