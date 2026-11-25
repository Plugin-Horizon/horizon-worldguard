//! horizon-worldguard as an arismoko native trait mod (`mod_api`). The rules are the unchanged
//! `worldguard-core` crate (`core/`, the former `api/src/wg/`); this file maps arismoko's hooks and
//! host calls onto them. Where arismoko has no API yet the line is marked
//! `TODO(arismoko-api: <name>)` and says what is lost meanwhile. PORT_NOTES.md lists them all.

pub mod worldguard_core;
mod chest_world;
use mod_api::{
    TriState, ArmorStandManipulateEvent, AttackEntityEvent, BlockBurnEvent, BlockIgniteEvent,
    BlockSpreadEvent, BreakBlockEvent, BucketEmptyEvent, CanPlayerSleepEvent, Command,
    CommandArgumentValue, CommandSource, Config, ConfigKind, ConfigLoadingEvent,
    ConfigReloadingEvent, EnderPearlTeleportEvent, EntityInteractEvent, EntityJoinLevelEvent,
    EntityLeaveLevelEvent, EntityMountEvent, EntityPlaceEvent, ExplosionDetonateEvent,
    FillBucketEvent, Flow, GameMode, HangingBreakEvent, HangingPlaceEvent, Hooks, HostSide,
    ItemPickedUpEvent, ItemTossEvent, LeavesDecayEvent, LevelHost, LevelRandomHost,
    LivingIncomingDamageEvent, PistonPreEvent, PlayerCommandEvent, PlayerLoggedInEvent,
    PlayerLoggedOutEvent, PlayerMoveEvent, PlayerRef, PlayerTeleportedEvent, RegisterCommandsEvent,
    RegisterConfigsEvent, RightClickBlockEvent, RunModCommandEvent, ServerChatEvent, ServerHost,
    ServerStoppingEvent, ServerTickEvent, ServerTickPhase, Suggestion, TabCompleteEvent, Text, TntPrimedEvent,
    InteractionResult, NodeDefault, PermissionGatherNodesEvent, PermissionLevel, PermissionNode, PermissionValueType,
    CropGrowPreEvent, EntityChangeBlockPreEvent, EntityMobGriefingEvent, ExplosionStartEvent,
    FluidPlaceBlockEvent, FoodLevelChangedEvent, Host, LivingExperienceDropEvent, LivingHealEvent, LoopContext,
    MoistureChangeEvent, BlockFadeEvent, BlockFormEvent, BlockGrowFeatureEvent, EntityStruckByLightningEvent, Hand,
    SignChangedEvent, WeatherChangeEvent, BlockDispenseEvent, BlockGrowEvent, EntityBreakDoorCancelEvent,
    NaturalBlockSpreadEvent, PlayerDeathMessageEvent, PotionSplashCancelEvent,
};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use wg::blacklist::{Blacklist, EventType};
use wg::commands::{self, Effect, Players};
use wg::config::{Config as WgConfig, DEFAULT_TOML};
use wg::geom::BlockPos;
use wg::guard::{Blast, Guard, Player};
use wg::movement::{MoveEvent, Tracker};
use wg::region::Subject;
use wg::selection::Selections;
use wg::store::World;
use wg::vitals::{Change, Vitals};
use wg::text;
use worldguard_core as wg;

const MOD_ID: &str = "worldguard";
/// Server ticks a second.
const TPS: u64 = 20;
/// Used for a dimension the hook does not name (the console's `/rg` and `/wg` reads it too).
const OVERWORLD: &str = "minecraft:overworld";

#[derive(Default)]
struct Mod {
    st: Mutex<State>,
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One online player, read through the host at the start of a tick (or when a hook needs it).
/// Field names are the ones the rules were written with.
#[derive(Clone)]
struct Snap {
    player: PlayerRef,
    entity_id: i32,
    uuid: u128,
    name: String,
    dimension: String,
    position: [f64; 3],
    yaw: f32,
    pitch: f32,
    health: f32,
    permission: u8,
    game_mode: i32,
}

struct State {
    guard: Guard,
    tracker: Tracker,
    selections: Box<dyn Selections + Send>,
    /// `<server>/<data-folder>`, known once a world is open.
    data_dir: Option<PathBuf>,
    loaded: bool,
    ticks: u64,
    /// When the dirty region files are written next.
    save_at: Option<u64>,
    /// The players as of the last tick, by entity id (for hooks that name an entity).
    by_entity: HashMap<i32, Snap>,
    by_uuid: HashMap<u128, Snap>,
    /// Names seen this session, lower case -> (uuid, name), for /rg addmember.
    names: HashMap<String, (u128, String)>,
    /// Console commands to run at the next tick (game modes).
    commands: Vec<String>,
    /// Messages to players for the next tick (from hooks that cannot write one now).
    tells: Vec<(u128, String)>,
    /// Entities that explode: id -> (kind, dimension).
    explosives: HashMap<u32, (String, String)>,
    /// Players who teleported since the last tick (exit-via-teleport).
    teleported: HashSet<u128>,
    /// Players in `/god` mode (damage to them is cancelled).
    god: HashSet<u128>,
    /// `heal-*` / `feed-*` timers.
    vitals: Vitals,
    /// Players who hold `worldguard.region.bypass.<world>` for the world they are in (read once a second).
    bypass: HashSet<u128>,
    /// `worlds/<world>/blacklist.txt`, by world folder name.
    blacklists: HashMap<String, Blacklist>,
    /// The weather last allowed per dimension (raining, thundering).
    weather: HashMap<String, (bool, bool)>,
    /// Where each player was, once a second (the last four), to move them out of the void or a wall.
    trail: HashMap<u128, VecDeque<(String, [f64; 3])>>,
    /// Players whose void or suffocation damage was cancelled and who are moved at the next tick.
    rescues: Vec<u128>,
    /// Sponges placed (dimension, position): the water around them is cleared at the next tick.
    sponge_clears: Vec<(String, [i32; 3])>,
    /// Who is under a `weather-lock` or `time-lock`.
    sessions: wg::session::Sessions,
}

impl Default for State {
    fn default() -> State {
        State {
            guard: Guard::new(WgConfig::default()),
            tracker: Tracker::default(),
            selections: new_selections(),
            data_dir: None,
            loaded: false,
            ticks: 0,
            save_at: None,
            by_entity: HashMap::new(),
            by_uuid: HashMap::new(),
            names: HashMap::new(),
            commands: Vec::new(),
            tells: Vec::new(),
            explosives: HashMap::new(),
            teleported: HashSet::new(),
            god: HashSet::new(),
            vitals: Vitals::default(),
            bypass: HashSet::new(),
            blacklists: HashMap::new(),
            weather: HashMap::new(),
            trail: HashMap::new(),
            rescues: Vec::new(),
            sponge_clears: Vec::new(),
            sessions: wg::session::Sessions::default(),
        }
    }
}

/// The player's WorldEdit selection (`worldedit::api`, the worldedit trait mod). Cuboids only: the
/// port of WorldEdit has no polygon selections. Each call takes WorldEdit's lock for its own
/// duration and calls nothing of ours while holding it.
struct WeSelections;

impl Selections for WeSelections {
    fn get(&self, player: u128) -> Option<wg::selection::Selection> {
        let (dimension, a, b) = worldedit::api::selection(player)?;
        Some(wg::selection::Selection::Cuboid {
            dimension,
            a: BlockPos::from(a),
            b: BlockPos::from(b),
        })
    }

    fn set(&mut self, player: u128, selection: wg::selection::Selection) -> bool {
        match selection {
            wg::selection::Selection::Cuboid { dimension, a, b } => {
                worldedit::api::select_cuboid(player, &dimension, a.arr(), b.arr());
                true
            }
            wg::selection::Selection::Polygon { .. } => false,
        }
    }
}

fn new_selections() -> Box<dyn Selections + Send> {
    Box::new(WeSelections)
}

impl State {
    fn region_file(&self, world: &str) -> Option<PathBuf> {
        Some(
            self.data_dir
                .as_ref()?
                .join("worlds")
                .join(world)
                .join("regions.yml"),
        )
    }

    /// Reads every world's region file once the server directory is known.
    fn load_all(&mut self, host: &dyn ServerHost) {
        if self.loaded {
            return;
        }
        let Some(world_dir) = host.world_dir() else {
            return;
        };
        let server = world_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(world_dir);
        let data = server.join(&self.guard.cfg.data_folder);
        self.data_dir = Some(data.clone());
        self.loaded = true;
        let mut names: Vec<String> = self.guard.cfg.worlds.values().cloned().collect();
        if let Ok(dirs) = std::fs::read_dir(data.join("worlds")) {
            for d in dirs.flatten() {
                if let Some(n) = d.file_name().to_str()
                    && !names.iter().any(|x| x == n)
                {
                    names.push(n.to_owned());
                }
            }
        }
        for n in names {
            self.load_world(&n);
        }
    }

    /// Reads a world's regions, its own `config.yml` and its `blacklist.txt`.
    fn load_world(&mut self, name: &str) -> String {
        let message = self.load_regions(name);
        self.load_world_options(name);
        message
    }

    /// `worlds/<world>/config.yml` (options for this world only) and `worlds/<world>/blacklist.txt`.
    fn load_world_options(&mut self, name: &str) {
        let Some(dir) = self.data_dir.as_ref().map(|d| d.join("worlds").join(name)) else {
            return;
        };
        match std::fs::read_to_string(dir.join("config.yml")) {
            Ok(text) => {
                if let Err(e) = self.guard.cfg.set_world_yaml(name, &text) {
                    eprintln!("[worldguard] {name}/config.yml is not readable, kept the old options: {e}");
                }
            }
            Err(_) => {
                self.guard.cfg.world_rules.remove(name);
            }
        }
        let whitelist = self.guard.cfg.rule_in(name, "blacklist.use-as-whitelist");
        match std::fs::read_to_string(dir.join("blacklist.txt")) {
            Ok(text) => {
                let (bl, warnings) = Blacklist::parse(&text, whitelist);
                for w in warnings {
                    eprintln!("[worldguard] {name}/blacklist.txt: {w}");
                }
                if bl.is_empty() {
                    self.blacklists.remove(name);
                } else {
                    self.blacklists.insert(name.to_owned(), bl);
                }
            }
            Err(_) => {
                self.blacklists.remove(name);
            }
        }
    }

    fn load_regions(&mut self, name: &str) -> String {
        let Some(file) = self.region_file(name) else {
            return "No world is open.".into();
        };
        match std::fs::read_to_string(&file) {
            Ok(text) => match World::load(name, &text) {
                Ok((w, problems)) => {
                    for p in &problems {
                        eprintln!("[worldguard] {}: {p}", file.display());
                    }
                    let n = w.all().filter(|r| !r.is_blank_global()).count();
                    self.guard.worlds.insert(name.to_owned(), w);
                    format!("Loaded {n} region(s) of '{name}'.")
                }
                Err(e) => {
                    // Keep what is in memory: an unreadable file is never overwritten by a save
                    // of nothing.
                    eprintln!(
                        "[worldguard] {} is not readable, not loaded: {e}",
                        file.display()
                    );
                    format!("The region file of '{name}' is not readable: {e}")
                }
            },
            Err(_) => {
                self.guard
                    .worlds
                    .entry(name.to_owned())
                    .or_insert_with(|| World::new(name));
                format!("No region file for '{name}' yet.")
            }
        }
    }

    fn save_world(&mut self, name: &str) {
        let Some(file) = self.region_file(name) else {
            return;
        };
        let Some(w) = self.guard.worlds.get_mut(name) else {
            return;
        };
        if w.is_empty() && !file.exists() {
            w.dirty = false;
            return;
        }
        let text = w.save();
        let tmp = file.with_extension("yml.tmp");
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&tmp, &text))
            .and_then(|()| std::fs::rename(&tmp, &file));
        match written {
            Ok(()) => w.dirty = false,
            Err(e) => eprintln!("[worldguard] could not write {}: {e}", file.display()),
        }
    }

    fn save_dirty(&mut self) {
        let dirty: Vec<String> = self
            .guard
            .worlds
            .iter()
            .filter(|(_, w)| w.dirty)
            .map(|(n, _)| n.clone())
            .collect();
        for n in dirty {
            self.save_world(&n);
        }
    }

    fn player(&self, p: &Snap) -> Player {
        Player {
            subject: Subject {
                uuid: p.uuid,
                name: p.name.clone(),
                groups: Vec::new(),
            },
            op_level: p.permission,
            dimension: p.dimension.clone(),
            pos: p.position,
            yaw: p.yaw,
            pitch: p.pitch,
            game_mode: p.game_mode,
            bypass_perm: self.bypass.contains(&p.uuid),
        }
    }

    fn cached(&self, uuid: u128) -> Option<Player> {
        self.by_uuid.get(&uuid).map(|p| self.player(p))
    }
}

/// One player through the host, or None when they are not in a level.
fn snap(host: &dyn ServerHost, p: PlayerRef) -> Option<Snap> {
    let entity = p.entity();
    let level = p.level(host)?;
    // `level.dimension().id(host)` is the name of the level's dimension.
    let dimension = level.dimension().id(host).unwrap_or_default();
    let position = entity.position(host)?;
    let [yaw, pitch] = entity.rotation(host).unwrap_or([0.0, 0.0]);
    // Live health through the living handle of the player's entity; 20 when there is none.
    let health = host
        .living(entity)
        .and_then(|l| l.health(host))
        .unwrap_or(20.0);
    let game_mode = match p.game_mode(host) {
        Some(GameMode::Creative) => 1,
        Some(GameMode::Adventure) => 2,
        Some(GameMode::Spectator) => 3,
        Some(GameMode::Survival) | None => 0,
    };
    Some(Snap {
        player: p,
        entity_id: entity.id() as i32,
        uuid: p.uuid(),
        name: p.name(host)?,
        dimension,
        position,
        yaw,
        pitch,
        health,
        permission: p.permission_level(host).unwrap_or(0),
        game_mode,
    })
}

/// Every online player that `snap` can read.
fn online(host: &dyn ServerHost) -> Vec<Snap> {
    host.players()
        .server_players()
        .into_iter()
        .filter_map(|p| snap(host, p))
        .collect()
}

/// A `&`-coded line as Text.
// TODO(arismoko-api: text_style): arismoko's TextStyle has no constructor for a bare colour, so
// the colour codes are dropped and the text is sent plain.
fn line(s: &str) -> Text {
    Text::literal(text::plain(s))
}

fn tell(host: &mut dyn ServerHost, uuid: u128, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(p) = host.player(uuid) {
        let _ = p.tell(host, line(s));
    }
}

/// Moves a player with `entity_teleport_to_level`; logs a refusal.
fn teleport(
    host: &mut dyn ServerHost,
    player: u128,
    dimension: &str,
    pos: [f64; 3],
    yaw: f32,
    pitch: f32,
) -> bool {
    let done = match host.player(player) {
        Some(p) => p
            .entity()
            .teleport_to_level(host, dimension, pos, Some([yaw, pitch])),
        None => false,
    };
    if !done {
        eprintln!(
            "[worldguard] teleport of {} to {dimension} {pos:?} refused",
            wg::region::uuid_string(player)
        );
    }
    done
}

/// `Overworld` / `Nether` / `End` (the level hooks' dimension, read through `Debug` because those
/// hooks get no host to ask) as an id.
// TODO(verify: LevelDimension Debug): the piston, fire, spread, burn and leaf-decay rules get
// their dimension from `LevelHost` hooks, and this maps the variant name. Until it is checked
// against the engine, a name that does not match means the rule does not apply there.
fn dim_id(d: &impl std::fmt::Debug) -> String {
    let s = format!("{d:?}");
    match s.as_str() {
        "Overworld" => "minecraft:overworld".into(),
        "Nether" => "minecraft:the_nether".into(),
        "End" => "minecraft:the_end".into(),
        other if other.contains('"') => {
            // `Custom(LevelKey("ns:name"))`: the text between the quotes.
            let from = other.find('"').map_or(0, |i| i + 1);
            let to = other.rfind('"').unwrap_or(other.len());
            other.get(from..to).unwrap_or(other).to_owned()
        }
        other if other.contains(':') => other.trim_matches('"').to_owned(),
        other => format!("minecraft:{}", other.to_ascii_lowercase()),
    }
}

/// A block's own position as the rules take it.
fn bp(p: [i32; 3]) -> BlockPos {
    BlockPos::new(p[0], p[1], p[2])
}

/// The same for a position that arrives as arismoko's `BlockPos`.
fn bp_of(p: impl Into<[i32; 3]>) -> BlockPos {
    let p: [i32; 3] = p.into();
    bp(p)
}

struct KnownPlayers<'a> {
    names: &'a HashMap<String, (u128, String)>,
    // The permission query needs the host mutably, and `Players` takes `&self`: the cell lends it for each query.
    host: RefCell<&'a mut dyn ServerHost>,
    op_level: u8,
}

/// The block name (`minecraft:stone`) of a state, "" when unknown.
fn state_name(level: &dyn LevelHost, state: mod_api::BlockState) -> String {
    level.state_block(state).unwrap_or("").to_owned()
}

/// The block name at `pos` through a level hook's `LevelHost`.
fn level_block(level: &dyn LevelHost, pos: [i32; 3]) -> String {
    level.block_state(pos).map(|s| state_name(level, s)).unwrap_or_default()
}

/// The block name at `pos` through the server host.
fn host_block(host: &dyn ServerHost, dimension: &str, pos: [i32; 3]) -> String {
    host.level(dimension)
        .and_then(|l| l.block(host, pos))
        .map(|k| k.id(host).to_string())
        .unwrap_or_default()
}

/// The dimensions of the worlds that have a region file, for a hook that names none.
fn region_dimensions(guard: &Guard) -> Vec<String> {
    guard
        .worlds
        .keys()
        .map(|w| guard.cfg.dimension_of(w).unwrap_or_else(|| format!("minecraft:{w}")))
        .collect()
}

/// The dimension of a hook that names none: the acting player's, else None.
fn acting_dimension(host: &dyn ServerHost) -> Option<String> {
    host.players()
        .acting_player()
        .and_then(|p| p.level(host))
        .map(|l| l.dimension_id().to_string())
}

/// `Entities.isIntensiveEntity`: items, primed TNT, experience orbs, falling blocks and every living thing that is not
/// tamed, a player or an armor stand.
fn is_intensive(host: &dyn ServerHost, entity: mod_api::EntityRef) -> bool {
    let Some(kind) = entity.kind(host).map(|k| k.id(host).to_string()) else {
        return false;
    };
    match kind.strip_prefix("minecraft:").unwrap_or(&kind) {
        "item" | "tnt" | "experience_orb" | "falling_block" => true,
        "player" | "armor_stand" => false,
        _ => host
            .living(entity)
            .is_some_and(|l| l.is_tamed(host) != Some(true)),
    }
}

/// Removes every intensive entity of every loaded level; the result is the count per level (by world name).
fn sweep_intensive(host: &mut dyn ServerHost, cfg: &WgConfig) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for level in host.levels().list_levels() {
        let world = cfg.world_of(&level.dimension_id().to_string());
        let Some(entities) = level.all_entities(&*host) else { continue };
        let mut removed = 0;
        for e in entities {
            if is_intensive(&*host, e) && e.discard(&mut *host) {
                removed += 1;
            }
        }
        out.push((world, removed));
    }
    out
}

/// Sends one play packet to `uuid` alone; false when it could not be built or sent.
fn send_to(host: &mut dyn ServerHost, uuid: u128, packet: &str, body: Vec<u8>) -> bool {
    use mod_api::{ClientboundPlayPacket, Identifier};
    let Ok(id) = Identifier::parse(packet) else { return false };
    let Some(player) = host.player(uuid) else { return false };
    host.player_send_packet(player, ClientboundPlayPacket::new(id, body)).is_ok()
}

/// One step of a `weather-lock` or `time-lock` for a player (see `worldguard_core::session`).
fn apply_lock(host: &mut dyn ServerHost, uuid: u128, dimension: &str, action: wg::session::Action) {
    use wg::session::{self, Action};
    let level = host.level(dimension);
    match action {
        Action::Weather(raining, thundering) => {
            for body in session::weather_events(raining, thundering) {
                send_to(host, uuid, "minecraft:game_event", body);
            }
        }
        Action::ResetWeather => {
            let (rain, thunder) = level.and_then(|l| l.weather(&*host)).unwrap_or((0.0, 0.0));
            for body in session::weather_level_events(rain, thunder) {
                send_to(host, uuid, "minecraft:game_event", body);
            }
        }
        Action::Time(..) | Action::ResetTime => {
            let Some(level) = level else { return };
            let (Ok(registry), Ok(name)) = (
                mod_api::Identifier::parse("minecraft:world_clock"),
                mod_api::Identifier::parse("minecraft:overworld"),
            ) else {
                return;
            };
            let Some(clock) = host.registry_protocol_id(&registry, &name) else { return };
            let now = level.clock_time(&*host).or_else(|| level.day_time(&*host)).unwrap_or(0);
            let game = level.game_time(&*host).unwrap_or(0);
            let body = match action {
                // A fixed time stops the client's clock; a relative one runs on from the world's time.
                Action::Time(t, true) => session::set_time_body(game, clock, now + t, 1.0),
                Action::Time(t, false) => session::set_time_body(game, clock, t, 0.0),
                _ => session::set_time_body(game, clock, now, 1.0),
            };
            send_to(host, uuid, "minecraft:set_time", body);
        }
    }
}

/// The typed node for one `worldguard.*` name (a Boolean, as the commands ask it).
fn perm_node(name: &str) -> PermissionNode {
    PermissionNode::new(name, PermissionValueType::Boolean)
}

/// Declares a region-scoped `worldguard.region.*` node after the server started, once per node (the host refuses a
/// second declaration with `DuplicateNode`, which is ignored). Default: operator level, as the gather-time nodes.
fn declare_region_node_once(host: &mut dyn ServerHost, node: &str) {
    static DECLARED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    if !node.starts_with("worldguard.region.") {
        return;
    }
    let mut seen = DECLARED.lock().unwrap_or_else(|e| e.into_inner());
    if seen.insert(node.to_owned()) {
        let _ = host.permission_declare_node(&perm_node(node), &NodeDefault::Level(PermissionLevel::from_level(2)));
    }
}

impl Players for KnownPlayers<'_> {
    // The node's answer from the permission system (LuckPerms answers through the `player_permission_check` hook);
    // a node that no one declared is refused, and so is any error: a denial, never a grant.
    // `worldguard.region.<perm>.<id>` (and `.own.`/`.member.`, `addowner.unclaimed.<id>`) depend on the region id, so
    // they cannot be declared at gather time, so they are declared late.
    // the per-region grants of non-operators. Each such node is declared the first time it is asked
    // (`permission_declare_node`). Operators still pass through `guard.is_admin` before these.
    fn has_permission(&self, uuid: u128, node: &str) -> bool {
        let mut slot = self.host.borrow_mut();
        let host: &mut dyn ServerHost = &mut **slot;
        declare_region_node_once(host, node);
        let Some(player) = host.player(uuid) else {
            return false;
        };
        player.permission_bool(host, &perm_node(node)).unwrap_or(false)
    }
    // Names seen this session first, then the server's own profile lookup (user cache; never Mojang).
    fn resolve(&self, name: &str) -> Option<(u128, String)> {
        self.names.get(&name.to_ascii_lowercase()).cloned().or_else(|| {
            let slot = self.host.borrow();
            slot.profile_by_name(name).map(|p| (p.uuid, p.name))
        })
    }
    fn name_of(&self, uuid: u128) -> Option<String> {
        self.names
            .values()
            .find(|(u, _)| *u == uuid)
            .map(|(_, n)| n.clone())
            .or_else(|| {
                let slot = self.host.borrow();
                slot.profile_by_uuid(uuid).map(|p| p.name)
            })
    }
}

impl Mod {
    fn load_config(&self, text: &str, what: &str) {
        match WgConfig::parse(text) {
            Ok(mut cfg) => {
                let mut st = lock(&self.st);
                // The worlds' own config files are not part of the common file.
                cfg.world_rules = std::mem::take(&mut st.guard.cfg.world_rules);
                st.guard.cfg = cfg;
                drop(st);
                eprintln!("[worldguard] config {what}");
            }
            Err(e) => eprintln!("[worldguard] config not {what}, keeping the old one: {e}"),
        }
    }

    /// Runs `/rg ...` for a player (`sender`) or the console; returns the reply lines.
    fn region_command(
        &self,
        host: &mut dyn ServerHost,
        sender: Option<&Snap>,
        args: &str,
    ) -> Vec<String> {
        let mut st = lock(&self.st);
        st.load_all(&*host);
        let player = sender.map(|p| st.player(p));
        let dimension = sender.map_or(OVERWORLD.to_owned(), |p| p.dimension.clone());
        // The current level's (min_y, height), as the rules' height limits want it.
        let (min_y, height) = host
            .level(&dimension)
            .and_then(|l| l.height(&*host))
            .unwrap_or((-64, 384));
        let height = (min_y, min_y + height - 1);
        let names = st.names.clone();
        let op_level = st.guard.cfg.op_level;
        let known = KnownPlayers {
            names: &names,
            host: RefCell::new(&mut *host),
            op_level,
        };
        let State {
            guard, selections, ..
        } = &mut *st;
        let mut ctx = commands::Ctx {
            sender: player.as_ref(),
            selections: selections.as_mut(),
            players: &known,
            height,
        };
        let reply = commands::run(guard, &mut ctx, args);
        let mut lines = reply.lines;
        let mut teleports = Vec::new();
        for e in reply.effects {
            match e {
                Effect::Teleport {
                    player,
                    region,
                    dimension,
                    pos,
                    yaw,
                    pitch,
                } => teleports.push((player, region, dimension, pos, yaw, pitch)),
                Effect::Save(w) => st.save_world(&w),
                Effect::Load(w) => lines.push(format!("&7{}", st.load_world(&w))),
            }
        }
        if st.guard.worlds.values().any(|w| w.dirty) && st.save_at.is_none() {
            st.save_at = Some(st.ticks + st.guard.cfg.save_delay_seconds as u64 * TPS);
        }
        // Not under the lock: a teleport may be heard by this mod's own teleport handling.
        drop(st);
        for (player, region, dimension, pos, yaw, pitch) in teleports {
            if !teleport(host, player, &dimension, pos, yaw, pitch) {
                lines.push(format!("&cUnable to teleport to region '{region}'."));
            }
        }
        lines
    }

    /// Runs the blacklist of the player's world for `kind` of `target` (a block or item id).
    /// Queues the player's message, the notices to operators, the log lines and a kick or ban;
    /// returns whether the action is allowed.
    fn blacklist_check(&self, host: &mut dyn ServerHost, p: &Player, kind: EventType, target: &str, silent: bool) -> bool {
        if target.is_empty() {
            return true;
        }
        let mut st = lock(&self.st);
        let world = st.guard.cfg.world_of(&p.dimension);
        let data_dir = st.data_dir.clone();
        let now_ms = st.ticks * 1000 / TPS;
        let State { blacklists, guard, by_uuid, tells, commands, .. } = &mut *st;
        let Some(bl) = blacklists.get_mut(&world) else {
            return true;
        };
        if !bl.has_target(target) {
            return true;
        }
        let player = host.player(p.subject.uuid);
        // `ignore-groups`: the groups the permission provider published for the player (none without a provider).
        let player_groups = host.permission_groups(p.subject.uuid).groups;
        let event = wg::blacklist::Event {
            kind,
            player: Some(p.subject.name.clone()),
            target: target.to_owned(),
        };
        // `ignore-perms`: a node that nobody declared is refused, so only declared nodes can exempt.
        let mut ignores = |groups: &[String], perms: &[String]| {
            groups.iter().any(|g| player_groups.iter().any(|pg| pg.eq_ignore_ascii_case(g.trim())))
                || player.is_some_and(|pl| {
                    perms
                        .iter()
                        .any(|n| pl.permission_bool(&mut *host, &perm_node(n.trim())).unwrap_or(false))
                })
        };
        let verdict = bl.check(&event, false, silent, now_ms, &mut ignores);
        let log_console = guard.rule(&p.dimension, "blacklist.logging.console.enable");
        for l in &verdict.log {
            if log_console {
                eprintln!("[worldguard] {l}");
            }
        }
        // `blacklist.logging.file.enable`: the same lines, into the daily file of the world's pattern.
        if guard.rule(&p.dimension, "blacklist.logging.file.enable") {
            if let Some(dir) = &data_dir {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let pattern = guard.cfg.text_in(&world, "blacklist.logging.file.path", wg::blacklist::LOG_FILE_PATTERN);
                // Keep the file under the data folder: no absolute path, no `..`.
                let pattern = if pattern.starts_with('/') || pattern.contains("..") { wg::blacklist::LOG_FILE_PATTERN } else { pattern };
                for l in &verdict.log {
                    if let Some(path) = wg::blacklist::log_file_path(pattern, &world, Some(&p.subject.name), now) {
                        append_log_line(&dir.join(path), &wg::blacklist::log_file_line(&p.subject.name, l, now));
                    }
                }
            }
        }
        for n in &verdict.notify {
            eprintln!("[worldguard] {n}");
            for s in by_uuid.values().filter(|s| s.permission >= guard.cfg.op_level) {
                tells.push((s.uuid, format!("&7{n}")));
            }
        }
        for t in &verdict.tell {
            tells.push((p.subject.uuid, format!("&c{t}")));
        }
        if let Some(reason) = &verdict.kick {
            commands.push(format!("kick {} {reason}", p.subject.name));
        }
        if let Some(reason) = &verdict.ban {
            commands.push(format!("ban {} {reason}", p.subject.name));
        }
        verdict.allowed
    }

    /// The blacklist's `on-dispense` for a dispenser (no player): whether `target` may be dispensed in `dimension`.
    fn dispense_allowed(&self, dimension: &str, target: &str) -> bool {
        let mut st = lock(&self.st);
        let world = st.guard.cfg.world_of(dimension);
        let now_ms = st.ticks * 1000 / TPS;
        let State { blacklists, guard, by_uuid, tells, .. } = &mut *st;
        let Some(bl) = blacklists.get_mut(&world) else {
            return true;
        };
        if !bl.has_target(target) {
            return true;
        }
        let event = wg::blacklist::Event { kind: EventType::Dispense, player: None, target: target.to_owned() };
        let verdict = bl.check(&event, false, false, now_ms, &mut |_, _| false);
        if guard.rule(dimension, "blacklist.logging.console.enable") {
            for l in &verdict.log {
                eprintln!("[worldguard] {l}");
            }
        }
        for n in &verdict.notify {
            eprintln!("[worldguard] {n}");
            for s in by_uuid.values().filter(|s| s.permission >= guard.cfg.op_level) {
                tells.push((s.uuid, format!("&7{n}")));
            }
        }
        verdict.allowed
    }

    /// `chest-protection.enable`: whether the block at `pos` is locked against `p` by a `[Lock]` sign.
    fn chest_locked(&self, host: &mut dyn ServerHost, p: &Player, pos: [i32; 3]) -> bool {
        if !lock(&self.st).guard.rule(&p.dimension, "chest-protection.enable") {
            return false;
        }
        let uuid = p.subject.uuid;
        if may(host, uuid, "worldguard.chest-protection.override", 0) || may(host, uuid, "worldguard.override.chest-protection", 0) {
            return false;
        }
        let reader = chest_world::HostReader { host: &*host, dimension: &p.dimension };
        wg::chest::is_protected(&reader, pos, Some(&p.subject.name))
    }

    /// The deny line for a player, and a cancel. A player is told at most twice a second
    /// (WorldGuard's 500 ms delay between deny messages); `%online%` and `%health%` are filled here.
    fn denied(host: &mut dyn ServerHost, p: &Player, msg: &str) -> Flow {
        static LAST: std::sync::OnceLock<Mutex<HashMap<u128, std::time::Instant>>> =
            std::sync::OnceLock::new();
        let now = std::time::Instant::now();
        let quiet = {
            let mut last = LAST
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let recent = last
                .get(&p.subject.uuid)
                .is_some_and(|t| now.duration_since(*t) < std::time::Duration::from_millis(500));
            if !recent {
                last.insert(p.subject.uuid, now);
            }
            recent
        };
        if !quiet && !msg.is_empty() {
            let online_count = host.players().server_players().len();
            let health = host
                .player(p.subject.uuid)
                .and_then(|pr| host.living(pr.entity()))
                .and_then(|l| l.health(&*host))
                .unwrap_or(20.0);
            let msg = msg
                .replace("%online%", &online_count.to_string())
                .replace("%health%", &format!("{health:?}"));
            tell(host, p.subject.uuid, &msg);
        }
        Flow::Cancel
    }

    /// Runs a protection check for the player `uuid` and tells them when it is denied.
    fn player_check(
        &self,
        host: &mut dyn ServerHost,
        uuid: u128,
        check: impl FnOnce(&Guard, &Player) -> wg::guard::Outcome,
    ) -> Flow {
        let st = lock(&self.st);
        let p = match host.players().acting_player().filter(|a| a.uuid() == uuid) {
            Some(ap) => match snap(&*host, ap) {
                Some(s) => st.player(&s),
                None => return Flow::Continue,
            },
            None => match st.cached(uuid) {
                Some(p) => p,
                None => return Flow::Continue,
            },
        };
        match check(&st.guard, &p) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
            }
        }
    }

    /// `/wg reload | flushstates [player] | running | version`.
    fn wg_command(&self, host: &mut dyn ServerHost, args: &str) -> Vec<String> {
        let mut st = lock(&self.st);
        st.load_all(&*host);
        let args = args.trim();
        let (sub, arg) = args
            .split_once(' ')
            .map_or((args, ""), |(s, a)| (s, a.trim()));
        match sub {
            "reload" => {
                let worlds: Vec<String> = st.guard.worlds.keys().cloned().collect();
                let mut out = Vec::new();
                for w in worlds {
                    out.push(format!("&7{}", st.load_world(&w)));
                }
                out.push("&dWorldGuard configuration reloaded.".to_owned());
                out
            }
            "flushstates" | "clearstates" => {
                if arg.is_empty() {
                    st.tracker.forget_all();
                    vec!["&dCleared all states.".into()]
                } else {
                    match st.names.get(&arg.to_ascii_lowercase()).map(|(u, _)| *u) {
                        Some(uuid) => {
                            st.tracker.forget(uuid);
                            vec![format!("&dCleared states for player \"{arg}\".")]
                        }
                        None => vec![format!("&cNo players matched query '{arg}'.")],
                    }
                }
            }
            "running" | "queue" => vec!["&dThere are currently no running tasks.".into()],
            "" | "version" => vec![
                // The Java plugin's version (gradle.properties of the WorldGuard source this port follows).
                "&dWorldGuard 7.0.22-SNAPSHOT".to_owned(),
                "&dhttp://www.enginehub.org".to_owned(),
                "&7----------- Platforms -----------".to_owned(),
                "&7* Horizon".to_owned(),
            ],
            "report" => {
                if arg.split_whitespace().any(|a| a == "-p") {
                    return vec!["&cThe report cannot be uploaded: there is no paste service.".into()];
                }
                let Some(dir) = st.data_dir.clone() else {
                    return vec!["&cFailed to write report: no world is open.".into()];
                };
                let mut report = String::from("WorldGuard report\n=================\nPlatform: Horizon\n\nWorlds\n------\n");
                for (name, w) in &st.guard.worlds {
                    report.push_str(&format!("{name}: {} region(s)\n", w.all().count()));
                }
                report.push_str("\nWorld options that are switched on\n----------------------------------\n");
                for (key, on) in &st.guard.cfg.rules {
                    if *on {
                        report.push_str(&format!("{key}\n"));
                    }
                }
                let file = dir.join("report.txt");
                match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, report)) {
                    Ok(()) => vec![format!(
                        "&dWorldGuard report written to {}/report.txt",
                        st.guard.cfg.data_folder.trim_end_matches('/')
                    )],
                    Err(e) => vec![format!("&cFailed to write report: {e}")],
                }
            }
            // HMOD-GAP(profiler): Horizon gives a mod no thread sampler and no way to replay events.
            "profile" | "stopprofile" | "debug" => {
                vec![format!("&c/wg {sub} is not available on Horizon.")]
            }
            _ => vec![
                "&cUnknown subcommand. Use /wg reload, flushstates, running, report, profile, stopprofile, debug or version.".into(),
            ],
        }
    }

    /// Nether portals (`player-portal` checks at the destination) are not ported: arismoko's
    /// `entity_portal` hook gives no cause and no destination, so the check below is kept but
    /// never called. TODO(arismoko-api: player_portal): cause and destination of a portal use.
    #[allow(dead_code)]
    fn player_portal_check(
        &self,
        st: &State,
        player: u128,
        cause: &str,
        to_dimension: &str,
    ) -> Option<String> {
        if cause != "nether_portal" || !st.guard.cfg.nether_portal_protection {
            return None;
        }
        let p = st.cached(player)?;
        // Overworld to Nether divides by 8, the way back multiplies: the portal is built there.
        let scale = if to_dimension.ends_with("the_nether") {
            0.125
        } else {
            8.0
        };
        let at = [p.pos[0] * scale, p.pos[1], p.pos[2] * scale];
        let dest = Player {
            dimension: to_dimension.to_owned(),
            pos: at,
            ..p
        };
        st.guard
            .place_block(&dest, BlockPos::of(at))
            .err()
            .map(|d| d.message)
    }

    /// `/rg ...`, `/wg ...` and their aliases, as typed by a player or the console.
    fn command_lines(
        &self,
        host: &mut dyn ServerHost,
        root: &str,
        sender: Option<&Snap>,
        args: &str,
    ) -> Vec<String> {
        match root {
            "rg" | "region" | "regions" => self.region_command(host, sender, args),
            "god" | "ungod" | "heal" | "slay" => self.general_command(host, root, sender, args),
            "stopfire" | "allowfire" => self.fire_command(host, root, sender, args),
            "halt-activity" | "stoplag" | "haltactivity" => self.halt_command(host, root, sender, args),
            "stack" => self.stack_command(host, sender),
            _ => {
                if let Some(s) = sender {
                    if let Some(node) = wg_node(args) {
                        let op_level = lock(&self.st).guard.cfg.op_level;
                        if !may(&mut *host, s.uuid, node, op_level) {
                            return vec![NO_PERMISSION.to_owned()];
                        }
                    }
                }
                self.wg_command(host, args)
            }
        }
    }

    /// The players a command names: `*` for everybody online, else a name (exact, or the only one
    /// that starts with it).
    fn match_targets(&self, query: &str) -> Result<Vec<(u128, String)>, String> {
        let st = lock(&self.st);
        if query == "*" {
            let all: Vec<(u128, String)> = st.by_uuid.values().map(|p| (p.uuid, p.name.clone())).collect();
            return if all.is_empty() { Err("&cNo players matched query.".to_owned()) } else { Ok(all) };
        }
        let q = query.to_ascii_lowercase();
        let online: Vec<&Snap> = st.by_uuid.values().collect();
        if let Some(p) = online.iter().find(|p| p.name.to_ascii_lowercase() == q) {
            return Ok(vec![(p.uuid, p.name.clone())]);
        }
        let partial: Vec<&&Snap> = online.iter().filter(|p| p.name.to_ascii_lowercase().starts_with(&q)).collect();
        match partial.as_slice() {
            [p] => Ok(vec![(p.uuid, p.name.clone())]),
            [] => Err(format!("&cNo players matched query '{query}'.")),
            _ => Err("&cMore than one player found! Use the exact name or '*'.".to_owned()),
        }
    }

    /// WorldGuard's `/god`, `/ungod`, `/heal`, `/slay` (own, or `<player>` / `*` with the `.other`
    /// node; `-s` keeps the targets from being told).
    fn general_command(
        &self,
        host: &mut dyn ServerHost,
        root: &str,
        sender: Option<&Snap>,
        args: &str,
    ) -> Vec<String> {
        let base = format!("worldguard.{}", if root == "ungod" { "god" } else { root });
        let op_level = lock(&self.st).guard.cfg.op_level;
        let mut silent = false;
        let mut words: Vec<&str> = Vec::new();
        for w in args.split_whitespace() {
            if w == "-s" {
                silent = true;
            } else if w.starts_with('-') && w.len() > 1 && !w[1..].starts_with(|c: char| c.is_ascii_digit()) {
                return vec![format!("&cUnknown flag: {}", &w[1..])];
            } else {
                words.push(w);
            }
        }
        if words.len() > 1 {
            return vec!["&cToo many parameters.".to_owned(), format!("&cUsage: /{root} [player]")];
        }
        let targets = match words.first() {
            None => {
                let Some(s) = sender else { return vec!["&cA player is expected.".to_owned()] };
                if !may(&mut *host, s.uuid, &base, op_level) {
                    return vec![NO_PERMISSION.to_owned()];
                }
                vec![(s.uuid, s.name.clone())]
            }
            Some(n) => {
                if let Some(s) = sender {
                    if !may(&mut *host, s.uuid, &format!("{base}.other"), op_level) {
                        return vec![NO_PERMISSION.to_owned()];
                    }
                }
                match self.match_targets(n) {
                    Ok(t) => t,
                    Err(e) => return vec![e],
                }
            }
        };
        let me = sender.map(|s| s.uuid);
        let sender_name = sender.map_or("Console".to_owned(), |s| s.name.clone());
        let mut included = false;
        let mut out: Vec<String> = Vec::new();
        for (uuid, name) in targets {
            let own = Some(uuid) == me;
            let (self_line, other_line) = match root {
                "god" => ("&dGod mode enabled! Use /ungod to disable.", format!("&dGod enabled by {sender_name}.")),
                "ungod" => ("&dGod mode disabled!", format!("&dGod disabled by {sender_name}.")),
                "heal" => ("&dHealed!", format!("&dHealed by {sender_name}.")),
                _ => ("&dSlain!", format!("&dSlain by {sender_name}.")),
            };
            match root {
                "god" => {
                    lock(&self.st).god.insert(uuid);
                }
                "ungod" => {
                    lock(&self.st).god.remove(&uuid);
                }
                "heal" => {
                    if let Some(player) = host.player(uuid) {
                        let max = host
                            .living(player.entity())
                            .and_then(|l| l.max_health(&*host))
                            .unwrap_or(20.0);
                        if let Some(living) = host.living(player.entity()) {
                            let _ = living.set_health(&mut *host, max);
                        }
                        let _ = player.set_food_level(&mut *host, 20);
                        let _ = player.set_saturation(&mut *host, 20.0);
                        let _ = player.set_exhaustion(&mut *host, 0.0);
                    }
                }
                _ => {
                    // The name comes from the players seen on this server, never from the typed text.
                    lock(&self.st).commands.push(format!("kill {name}"));
                }
            }
            if own {
                out.push(self_line.to_owned());
                included = true;
            } else if !silent {
                tell(host, uuid, &other_line);
            }
        }
        if !included {
            out.push(
                match root {
                    "god" => "&dPlayers now have god mode.",
                    "ungod" => "&dPlayers no longer have god mode.",
                    "heal" => "&dPlayers healed.",
                    _ => "&dPlayers slain.",
                }
                .to_owned(),
            );
        }
        out
    }

    /// `broadcastNotification`: the console log and the operators online.
    fn notify(&self, host: &mut dyn ServerHost, line: &str) {
        eprintln!("[worldguard] {}", text::plain(line));
        let op_level = lock(&self.st).guard.cfg.op_level;
        for p in online(&*host).iter().filter(|p| p.permission >= op_level) {
            tell(host, p.uuid, line);
        }
    }

    /// `/stopfire [world]` and `/allowfire [world]`.
    fn fire_command(
        &self,
        host: &mut dyn ServerHost,
        root: &str,
        sender: Option<&Snap>,
        args: &str,
    ) -> Vec<String> {
        let op_level = lock(&self.st).guard.cfg.op_level;
        if let Some(s) = sender {
            if !may(&mut *host, s.uuid, "worldguard.fire-toggle.stop", op_level) {
                return vec![NO_PERMISSION.to_owned()];
            }
        }
        let words: Vec<&str> = args.split_whitespace().collect();
        if words.len() > 1 {
            return vec!["&cToo many parameters.".to_owned(), format!("&cUsage: /{root} [<world>]")];
        }
        let world = match words.first() {
            Some(w) => {
                let st = lock(&self.st);
                match st.guard.cfg.worlds.values().find(|n| n.eq_ignore_ascii_case(w)) {
                    Some(n) => n.clone(),
                    None => return vec!["&cNo world by that exact name found.".to_owned()],
                }
            }
            None => match sender {
                Some(s) => lock(&self.st).guard.cfg.world_of(&s.dimension),
                None => return vec!["&cA player is expected.".to_owned()],
            },
        };
        let stop = root == "stopfire";
        let name = sender.map_or("Console".to_owned(), |s| s.name.clone());
        let changed = {
            let mut st = lock(&self.st);
            if stop {
                st.guard.fire_stopped.insert(world.clone())
            } else {
                st.guard.fire_stopped.remove(&world)
            }
        };
        if changed {
            let line = if stop {
                format!("&eFire spread has been globally disabled for '{world}' by {name}.")
            } else {
                format!("&eFire spread has been globally for '{world}' re-enabled by {name}.")
            };
            self.notify(host, &line);
            Vec::new()
        } else if stop {
            vec!["&dFire spread was already globally disabled.".to_owned()]
        } else {
            vec!["&dFire spread was already globally enabled.".to_owned()]
        }
    }

    /// `/halt-activity [confirm]` (`/stoplag`, `/haltactivity`) with `-c` (lift), `-i` (status), `-s` (silent).
    // The entities that are there are removed (`sweep_intensive`), and again every ten seconds while the halt
    // lasts (WorldGuard does it on every chunk load and join); spawns, explosions, fire, leaf decay and fluid flow
    // are stopped.
    fn halt_command(
        &self,
        host: &mut dyn ServerHost,
        root: &str,
        sender: Option<&Snap>,
        args: &str,
    ) -> Vec<String> {
        let op_level = lock(&self.st).guard.cfg.op_level;
        if let Some(s) = sender {
            if !may(&mut *host, s.uuid, "worldguard.halt-activity", op_level) {
                return vec![NO_PERMISSION.to_owned()];
            }
        }
        let (mut lift, mut status, mut silent) = (false, false, false);
        let mut words: Vec<&str> = Vec::new();
        for w in args.split_whitespace() {
            match w {
                "-c" => lift = true,
                "-i" => status = true,
                "-s" => silent = true,
                "-cis" | "-csi" | "-ics" | "-isc" | "-sci" | "-sic" | "-ci" | "-ic" | "-cs" | "-sc" | "-is" | "-si" => {
                    lift |= w.contains('c');
                    status |= w.contains('i');
                    silent |= w.contains('s');
                }
                _ => words.push(w),
            }
        }
        if words.len() > 1 {
            return vec!["&cToo many parameters.".to_owned(), format!("&cUsage: /{root} [confirm]")];
        }
        let halted = lock(&self.st).guard.activity_halted;
        if status {
            return vec![if halted {
                "&dALL intensive server activity is not allowed.".to_owned()
            } else {
                "&dALL intensive server activity is allowed.".to_owned()
            }];
        }
        let halt = !lift;
        if halt && !words.first().is_some_and(|w| w.eq_ignore_ascii_case("confirm")) {
            return vec![
                "&cThis command will &c&lPERMANENTLY&r&c erase ALL animals in ALL loaded chunks in ALL loaded worlds.".to_owned(),
                format!("&aType &f/{root} confirm&c to confirm."),
            ];
        }
        lock(&self.st).guard.activity_halted = halt;
        let name = sender.map_or("Console".to_owned(), |s| s.name.clone());
        let mut out = Vec::new();
        if halt {
            if sender.is_none() {
                out.push("&dALL intensive server activity halted.".to_owned());
            }
            if silent {
                out.push(format!("&d(Silent) ALL intensive server activity halted by {name}."));
            } else {
                self.notify(host, &format!("&eALL intensive server activity halted by {name}."));
            }
            let cfg = lock(&self.st).guard.cfg.clone();
            for (world, removed) in sweep_intensive(&mut *host, &cfg) {
                if removed > 10 {
                    out.push(format!("{removed} entities (>10) auto-removed from {world}"));
                }
            }
        } else if silent {
            out.push("&d(Silent) ALL intensive server activity is now allowed.".to_owned());
        } else {
            self.notify(host, "&eALL intensive server activity is now allowed.");
            if sender.is_none() {
                out.push("&dALL intensive server activity is now allowed.".to_owned());
            }
        }
        out
    }

    /// `/stack`: merges the player's stacks of the same item (main inventory and hotbar).
    fn stack_command(&self, host: &mut dyn ServerHost, sender: Option<&Snap>) -> Vec<String> {
        let Some(s) = sender else { return vec!["&cA player is expected.".to_owned()] };
        let op_level = lock(&self.st).guard.cfg.op_level;
        if !may(&mut *host, s.uuid, "worldguard.stack", op_level) {
            return vec![NO_PERMISSION.to_owned()];
        }
        let Some(mut slots) = s.player.inventory_slots(&*host) else {
            return vec!["&cYour inventory cannot be read.".to_owned()];
        };
        slots.retain(|(slot, _)| *slot < 36);
        slots.sort_by_key(|(slot, _)| *slot);
        let mut firsts: Vec<usize> = Vec::new();
        let mut entries: Vec<wg::stack::Entry> = Vec::new();
        for (i, (slot, stack)) in slots.iter().enumerate() {
            let group = match firsts.iter().position(|f| slots[*f].1.is_stackable_with(stack)) {
                Some(g) => g,
                None => {
                    firsts.push(i);
                    firsts.len() - 1
                }
            };
            entries.push(wg::stack::Entry {
                slot: *slot,
                group,
                count: stack.count(),
                max: host.max_stack_size(stack.item()).unwrap_or(64),
            });
        }
        for (slot, count) in wg::stack::compact(&entries) {
            let Some((_, original)) = slots.iter().find(|(n, _)| *n == slot) else { continue };
            let mut stack = original.clone();
            let replacement = if count == 0 {
                None
            } else if stack.set_count(count).is_ok() {
                Some(stack)
            } else {
                continue;
            };
            let _ = s.player.set_inventory_slot(&mut *host, slot, replacement);
        }
        vec!["&dItems compacted into stacks!".to_owned()]
    }
}

/// Appends one line to a blacklist log file, creating its folder (`FileHandler.log`). A failed write goes to the
/// console and is otherwise ignored, as the original does.
fn append_log_line(path: &Path, line: &str) {
    use std::io::Write;
    let res = path.parent().map_or(Ok(()), |dir| std::fs::create_dir_all(dir)).and_then(|()| {
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(line.as_bytes())
    });
    if let Err(e) = res {
        eprintln!("[worldguard] Could not write the blacklist log: {e}");
    }
}

const NO_PERMISSION: &str = "&cYou don't have permission to do that.";

/// The node a `/wg` sub-command needs (`None`: open to everybody, the version).
fn wg_node(args: &str) -> Option<&'static str> {
    match args.split_whitespace().next().unwrap_or("").to_ascii_lowercase().as_str() {
        "" | "version" => None,
        "flushstates" | "clearstates" => Some("worldguard.flushstates"),
        "running" | "queue" => Some("worldguard.running"),
        "report" => Some("worldguard.report"),
        "profile" | "stopprofile" => Some("worldguard.profile"),
        "debug" => Some("worldguard.debug.event"),
        "reload" => Some("worldguard.reload"),
        // An unknown word is answered with the list of subcommands, to everybody.
        _ => None,
    }
}

// --- slash forms ----------------------------------------------------------------------------------
//
// WorldGuard's commands (`/god`, `/rg`, `/wg`...) exist with ONE slash only; `//god` is not a command. The
// player-command hook hears the line without its slashes, and the engine builds so far trimmed ALL of them, so the two
// look alike. An engine that trims only one slash delivers `/god` for a typed `//god`: from the first line that begins
// with `/` the double-slash forms of our roots are refused as unknown.

static SLASH_AWARE: AtomicBool = AtomicBool::new(false);

const OUR_ROOTS: &[&str] = &[
    "rg", "region", "regions", "wg", "worldguard", "god", "ungod", "heal", "slay", "stopfire", "allowfire",
    "halt-activity", "stoplag", "haltactivity", "stack",
];

// --- completions ----------------------------------------------------------------------------------
//
// `/rg` and `/wg` are one `<args:text>` argument for the engine, so the words come from the `tab_complete` hook: the
// sub-commands, the options, the flag names, the region ids of the player's world (or of `-w <world>`), the worlds,
// the online players and the values of state flags (`allow`, `deny`).

/// `commands::usages()` split into words, built once. The first word is the sub-command.
fn rg_usage_words() -> &'static Vec<Vec<String>> {
    static WORDS: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    WORDS.get_or_init(|| commands::usages().iter().map(|u| u.split_whitespace().map(str::to_owned).collect()).collect())
}

/// A usage word `<name:kind>` or `[name:kind]`: its name and kind.
fn usage_arg(word: &str) -> Option<(&str, &str)> {
    word.strip_prefix('<')
        .and_then(|w| w.strip_suffix('>'))
        .or_else(|| word.strip_prefix('[').and_then(|w| w.strip_suffix(']')))?
        .split_once(':')
}

fn starts_with_any_case(word: &str, prefix: &str) -> bool {
    word.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase())
}

/// Whether the player may use `/rg <sub>` at all: an operator, a `player-commands` entry, or a provider's grant of
/// `worldguard.region.<sub>` (also its `.own` / `.member` forms, which apply to regions the player stands in).
fn may_rg_sub(host: &mut dyn ServerHost, uuid: u128, op_level: u8, player_commands: &[String], sub: &str) -> bool {
    let Some(p) = host.player(uuid) else { return false };
    if p.permission_level(&*host).unwrap_or(0) >= op_level || player_commands.iter().any(|c| c == sub) {
        return true;
    }
    ["", ".own", ".member"].iter().any(|tail| {
        p.permission_bool(&mut *host, &perm_node(&format!("worldguard.region.{sub}{tail}"))).unwrap_or(false)
    })
}

impl Mod {
    /// Re-reads `worldguard.region.bypass.<world>` for every online player. The node's default is
    /// the operator level, so operators hold it unless a permission plugin says otherwise.
    fn refresh_bypass(&self, host: &mut dyn ServerHost) {
        let players = online(&*host);
        let worlds: Vec<String> = {
            let st = lock(&self.st);
            players.iter().map(|p| st.guard.cfg.world_of(&p.dimension)).collect()
        };
        let mut holders = HashSet::new();
        for (np, world) in players.iter().zip(worlds) {
            let node = perm_node(&format!("worldguard.region.bypass.{world}"));
            if np.player.permission_bool(&mut *host, &node).unwrap_or(false) {
                holders.insert(np.uuid);
            }
        }
        lock(&self.st).bypass = holders;
    }

    /// The words that complete the last word of `rest` (what follows `/rg `).
    fn complete_region(&self, host: &mut dyn ServerHost, uuid: u128, rest: &str) -> Vec<String> {
        let tokens: Vec<&str> = rest.split(' ').collect();
        let n = tokens.len() - 1;
        let prefix = tokens[n];
        let (op_level, player_commands, world_of_player) = {
            let st = lock(&self.st);
            let dimension = host
                .player(uuid)
                .and_then(|p| p.entity().level(&*host))
                .map(|l| l.dimension_id().to_string())
                .unwrap_or_else(|| OVERWORLD.to_owned());
            (st.guard.cfg.op_level, st.guard.cfg.player_commands.clone(), st.guard.cfg.world_of(&dimension))
        };
        let mut out: Vec<String> = Vec::new();
        let add = |out: &mut Vec<String>, word: &str| {
            if starts_with_any_case(word, prefix) && !out.iter().any(|w| w == word) {
                out.push(word.to_owned());
            }
        };
        let usages = rg_usage_words();
        if n == 0 {
            for u in usages {
                if may_rg_sub(host, uuid, op_level, &player_commands, &u[0]) {
                    add(&mut out, &u[0]);
                }
            }
            return out;
        }
        let Some(sub) = commands::canonical(tokens[0]) else { return out };
        if !may_rg_sub(host, uuid, op_level, &player_commands, sub) {
            return out;
        }
        // `-w <world>` typed before the cursor names the world the region ids come from.
        let world = tokens[..n]
            .windows(2)
            .find(|w| w[0].eq_ignore_ascii_case("-w"))
            .map(|w| w[1].to_owned())
            .unwrap_or(world_of_player);
        for usage in usages {
            if usage[0] != sub {
                continue;
            }
            for i in 1..=n {
                let Some(word) = usage.get(i) else { break };
                let arg = usage_arg(word);
                if i < n {
                    // A word typed before the cursor has to fit the usage's word at that place.
                    let typed = tokens[i];
                    let fits = match arg {
                        None => typed.eq_ignore_ascii_case(word),
                        Some((_, "integer")) => typed.parse::<i64>().is_ok(),
                        Some(_) => true,
                    };
                    if !fits {
                        break;
                    }
                    continue;
                }
                match arg {
                    None => add(&mut out, word),
                    Some((name, kind)) => match (name, kind) {
                        ("id", _) | ("parent", _) => {
                            let st = lock(&self.st);
                            if let Some(w) = st.guard.worlds.get(&world) {
                                for r in w.all() {
                                    add(&mut out, &r.id);
                                }
                            }
                            add(&mut out, "__global__");
                        }
                        ("world", _) => {
                            let st = lock(&self.st);
                            for w in st.guard.worlds.keys() {
                                add(&mut out, w);
                            }
                        }
                        ("group", _) => {
                            for g in ["all", "members", "owners", "nonmembers", "nonowners", "none"] {
                                add(&mut out, g);
                            }
                        }
                        ("value", _) => {
                            // `flag <id> <flag-name> [-g <group>] [value]`: the flag's own value words.
                            let flag = usage.get(2).and_then(|f| wg::flags::FLAGS.iter().find(|d| d.name == f.as_str()));
                            if let Some(def) = flag {
                                let values: &[&str] = match def.kind {
                                    wg::flags::Kind::State => &["allow", "deny"],
                                    wg::flags::Kind::Bool => &["true", "false"],
                                    wg::flags::Kind::GameMode => &["survival", "creative", "adventure", "spectator"],
                                    _ => &[],
                                };
                                for v in values {
                                    add(&mut out, v);
                                }
                            }
                        }
                        ("player", _) | ("others", _) | ("owners", _) | ("names", _) => {
                            for p in host.players().server_players() {
                                if let Some(name) = p.name(host) {
                                    add(&mut out, &name);
                                }
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        out
    }

    /// The words that complete the last word of `rest` (what follows `/wg `).
    fn complete_wg(&self, host: &mut dyn ServerHost, uuid: u128, rest: &str) -> Vec<String> {
        if rest.contains(' ') {
            return Vec::new();
        }
        let op_level = lock(&self.st).guard.cfg.op_level;
        ["version", "reload", "flushstates", "running", "report", "profile", "stopprofile", "debug"]
            .iter()
            .filter(|sub| wg_node(sub).is_none_or(|node| may(&mut *host, uuid, node, op_level)))
            .filter(|sub| starts_with_any_case(sub, rest))
            .map(|sub| (*sub).to_owned())
            .collect()
    }
}

/// The node's answer from the permission system: its declared default is the operator level (`permission_gather_nodes`),
/// so the level fallback is in the answer itself. An undeclared node or an error is a denial.
fn may(host: &mut dyn ServerHost, uuid: u128, node: &str, _op_level: u8) -> bool {
    host.player(uuid).is_some_and(|p| p.permission_bool(&mut *host, &perm_node(node)).unwrap_or(false))
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn permission_gather_nodes(&self, event: &mut PermissionGatherNodesEvent<'_>) {
        // Every node known before any region exists. Their default is the operator level the string nodes fell back to
        // (`op-permission-level`, 2 by default). Region-scoped nodes cannot be declared here (see `KnownPlayers`).
        let mut names: BTreeSet<String> = [
            "region.define",
            "region.claim",
            "region.list",
            "region.list.own",
            "region.migrateheights",
            "region.migratedb",
            "region.migrateuuid",
            "region.load",
            "region.save",
            "region.unlimited",
            "region.toggle-bypass",
            "flushstates",
            "running",
            "report",
            "reload",
            "god",
            "heal",
            "slay",
        ]
        .iter()
        .map(|n| format!("worldguard.{n}"))
        .collect();
        for n in ["god", "heal", "slay"] {
            names.insert(format!("worldguard.{n}.other"));
        }
        for n in [
            "stack",
            "profile",
            "halt-activity",
            "fire-toggle.stop",
            "report.pastebin",
            "debug.event",
            "notify",
            "chest-protection.override",
            "override.chest-protection",
            "override.lighter",
        ] {
            names.insert(format!("worldguard.{n}"));
        }
        // `worldguard.region.bypass.<world>`: one per world the config names.
        let mut worlds: BTreeSet<String> = ["world", "world_nether", "world_the_end"].iter().map(|w| (*w).to_owned()).collect();
        worlds.extend(lock(&self.st).guard.cfg.worlds.values().cloned());
        for w in worlds {
            names.insert(format!("worldguard.region.bypass.{w}"));
        }
        // `/rg` subcommands: `worldguard.region.<sub>` and its `.own` / `.member` forms.
        for sub in commands::usages().iter().filter_map(|u| u.split_whitespace().next()) {
            for tail in ["", ".own", ".member"] {
                names.insert(format!("worldguard.region.{sub}{tail}"));
            }
        }
        for name in names {
            let _ = event.nodes.add(perm_node(&name), NodeDefault::Level(PermissionLevel::from_level(2)));
        }
    }

    fn register_configs(&self, _: HostSide, event: &mut RegisterConfigsEvent<'_>) {
        event
            .configs
            .push(Config::new(MOD_ID, ConfigKind::Common, DEFAULT_TOML));
    }

    fn config_loading(&self, _: HostSide, event: &mut ConfigLoadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            self.load_config(&event.config.text, "loaded");
        }
    }

    fn config_reloading(&self, _: HostSide, event: &mut ConfigReloadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            self.load_config(&event.config.text, "reloaded");
        }
    }

    fn server_stopping(&self, _host: &mut dyn ServerHost, _event: &mut ServerStoppingEvent) {
        lock(&self.st).save_dirty();
    }

    fn register_commands(&self, _host: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        // One text argument per root: arismoko's dispatcher hands the typed line to
        // `run_mod_command`, which runs the subcommand. `/rg` keeps level 0 (non-operators may run
        // what `player-commands` lists; `run` checks the rest). `/wg` is level 2.
        for root in ["rg", "region", "regions"] {
            event.commands.push(Command::new(root));
            event
                .commands
                .push(Command::new(&format!("{root} <args:text>")));
        }
        // `/wg` and the general commands are level 0 too: every path checks its node in
        // `command_lines` (wg_node / general_command) before anything runs.
        for root in [
            "wg", "worldguard", "god", "ungod", "heal", "slay", "stopfire", "allowfire", "halt-activity", "stoplag",
            "haltactivity", "stack",
        ] {
            event.commands.push(Command::new(root));
            event
                .commands
                .push(Command::new(&format!("{root} <args:text>")));
        }
    }

    fn run_mod_command(
        &self,
        host: &mut dyn ServerHost,
        event: &mut RunModCommandEvent<'_>,
    ) -> Flow {
        let Some(root) = event.command.literals.first().cloned() else {
            return Flow::Continue;
        };
        if !matches!(
            root.as_str(),
            "rg" | "region" | "regions" | "wg" | "worldguard" | "god" | "ungod" | "heal" | "slay" | "stopfire"
                | "allowfire" | "halt-activity" | "stoplag" | "haltactivity" | "stack"
        ) {
            return Flow::Continue;
        }
        let args = match event.command.argument("args") {
            Some(CommandArgumentValue::String(s)) => s.clone(),
            _ => String::new(),
        };
        match *event.source {
            CommandSource::Player { player, .. } => {
                let Some(sender) = snap(&*host, player) else {
                    return Flow::Cancel;
                };
                for l in self.command_lines(host, &root, Some(&sender), &args) {
                    tell(host, sender.uuid, &l);
                }
            }
            _ => {
                for l in self.command_lines(host, &root, None, &args) {
                    let _ = host.commands().feedback(line(&l));
                }
            }
        }
        Flow::Cancel
    }

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        if !matches!(
            event.phase,
            ServerTickPhase::ServerTickStart {
                world_running: true,
                ..
            }
        ) {
            return;
        }
        // `worldguard.region.bypass.<world>`, asked once a second, before the lock is taken.
        if lock(&self.st).ticks % TPS == 0 {
            self.refresh_bypass(&mut *host);
        }
        let mut st = lock(&self.st);
        st.load_all(&*host);
        st.ticks += 1;
        let players = online(&*host);
        st.by_entity = players.iter().map(|p| (p.entity_id, p.clone())).collect();
        st.by_uuid = players.iter().map(|p| (p.uuid, p.clone())).collect();
        for p in &players {
            st.names
                .insert(p.name.to_ascii_lowercase(), (p.uuid, p.name.clone()));
        }
        let teleported = std::mem::take(&mut st.teleported);
        let online_count = players.len();
        let mut tells: Vec<(u128, String)> = std::mem::take(&mut st.tells);
        let mut titles: Vec<(u128, String, String)> = Vec::new();
        let mut pushes: Vec<(u128, String, [f64; 3], f32, f32)> = Vec::new();
        let mut ops_notes: Vec<String> = Vec::new();
        let mut vital_changes: Vec<(u128, Change)> = Vec::new();
        let mut lock_actions: Vec<(u128, String, wg::session::Action)> = Vec::new();
        let now_ms = st.ticks * 1000 / TPS;
        for np in &players {
            let p = st.player(np);
            let world = st.guard.cfg.world_of(&p.dimension);
            let fill = |s: &str, id: &str| {
                text::fill(
                    s,
                    &text::Placeholders {
                        name: &np.name,
                        world: &world,
                        online: online_count,
                        id,
                        health: np.health,
                    },
                )
            };
            let max_health = host
                .living(np.player.entity())
                .and_then(|l| l.max_health(&*host))
                .unwrap_or(20.0);
            let food = np.player.food(&*host).map_or(20, |f| f.level);
            let State { guard, tracker, vitals, .. } = &mut *st;
            for c in vitals.step(guard, &p, np.health, max_health, food, now_ms) {
                vital_changes.push((np.uuid, c));
            }
            // Backstop: this tick's check pushes back what `player_move` did not refuse (PORT_NOTES.md).
            let events = tracker.update(
                guard,
                &p,
                np.yaw,
                np.pitch,
                teleported.contains(&np.uuid),
                &fill,
            );
            for e in events {
                match e {
                    MoveEvent::Tell(s) => tells.push((np.uuid, s)),
                    MoveEvent::Title(a, b) => titles.push((np.uuid, a, b)),
                    MoveEvent::PushBack { to, yaw, pitch } => {
                        pushes.push((np.uuid, np.dimension.clone(), to, yaw, pitch))
                    }
                    MoveEvent::Notify(s) => ops_notes.push(s),
                    MoveEvent::GameMode(m) => {
                        let mode = ["survival", "creative", "adventure", "spectator"]
                            [m.clamp(0, 3) as usize];
                        st.commands.push(format!("gamemode {mode} {}", np.name));
                    }
                }
            }
        }
        // `weather-lock` and `time-lock`: what each player sees while a region locks it.
        {
            let ps: Vec<Player> = players.iter().map(|np| st.player(np)).collect();
            let State { guard, sessions, .. } = &mut *st;
            for (np, p) in players.iter().zip(&ps) {
                let weather = guard.lock_value(p, "weather-lock");
                let time = guard.lock_value(p, "time-lock");
                for a in sessions.step(np.uuid, weather.as_deref(), time.as_deref()) {
                    lock_actions.push((np.uuid, np.dimension.clone(), a));
                }
            }
        }
        // One position a second per player, the last four: where `teleport-on-void-falling` and
        // `teleport-on-suffocation` put a player back.
        let online_set: HashSet<u128> = players.iter().map(|p| p.uuid).collect();
        st.trail.retain(|u, _| online_set.contains(u));
        if st.ticks % TPS == 0 {
            for np in &players {
                if np.position[1] > -60.0 {
                    let t = st.trail.entry(np.uuid).or_default();
                    t.push_back((np.dimension.clone(), np.position));
                    while t.len() > 4 {
                        t.pop_front();
                    }
                }
            }
        }
        for uuid in std::mem::take(&mut st.rescues) {
            let Some(np) = st.by_uuid.get(&uuid).cloned() else { continue };
            if let Some((dim, pos)) = st.trail.get(&uuid).and_then(|t| t.front().cloned()) {
                pushes.push((uuid, dim, pos, np.yaw, np.pitch));
            }
        }
        // `simulation.sponge`: the water around a sponge that was placed.
        for (dim, at) in std::mem::take(&mut st.sponge_clears) {
            let r = st.guard.sponge_radius(&dim);
            st.commands.push(format!(
                "execute in {dim} run fill {} {} {} {} {} {} minecraft:air replace minecraft:water",
                at[0] - r, at[1] - r, at[2] - r, at[0] + r, at[1] + r, at[2] + r
            ));
        }
        let cmds = std::mem::take(&mut st.commands);
        let op_level = st.guard.cfg.op_level;
        if st.save_at.is_some_and(|t| st.ticks >= t) {
            st.save_at = None;
            st.save_dirty();
        }
        let sweep = st.guard.activity_halted && st.ticks % (TPS * 10) == 0;
        let sweep_cfg = if sweep { Some(st.guard.cfg.clone()) } else { None };
        drop(st);
        if let Some(cfg) = sweep_cfg {
            // `/halt-activity`: what loaded or spawned in since the last sweep (WorldGuard does it per chunk load).
            let removed: usize = sweep_intensive(host, &cfg).iter().map(|(_, n)| n).sum();
            if removed > 50 {
                eprintln!("[worldguard] Halt-Act: {removed} entities (>50) auto-removed");
            }
        }
        for (u, dim, to, yaw, pitch) in pushes {
            teleport(host, u, &dim, to, yaw, pitch);
        }
        for (u, dim, action) in lock_actions {
            apply_lock(host, u, &dim, action);
        }
        for (u, change) in vital_changes {
            let Some(player) = host.player(u) else { continue };
            match change {
                Change::Health(h) => {
                    if let Some(living) = host.living(player.entity()) {
                        let _ = living.set_health(&mut *host, h);
                    }
                }
                Change::Food(n) => {
                    let _ = player.set_food_level(&mut *host, n);
                    let _ = player.set_saturation(&mut *host, n as f32);
                }
            }
        }
        for c in cmds {
            if !host.commands().run_command(&c) {
                eprintln!("[worldguard] command failed: {c}");
            }
        }
        for (u, s) in tells {
            tell(host, u, &s);
        }
        for (u, a, b) in titles {
            // TODO(arismoko-api: player_title): no title on arismoko; the action bar carries both lines.
            let text = if b.is_empty() {
                a
            } else {
                format!("{a} - {b}")
            };
            if let Some(p) = host.player(u) {
                let _ = p.action_bar(host, line(&text));
            }
        }
        if !ops_notes.is_empty() {
            for n in &ops_notes {
                eprintln!("[worldguard] {}", text::plain(n));
            }
            for p in players.iter().filter(|p| p.permission >= op_level) {
                for n in &ops_notes {
                    tell(host, p.uuid, n);
                }
            }
        }
    }

    fn player_logged_in(&self, host: &mut dyn ServerHost, event: &mut PlayerLoggedInEvent<'_>) {
        let mut st = lock(&self.st);
        st.names.insert(
            event.name.to_ascii_lowercase(),
            (event.player, event.name.to_owned()),
        );
        if st.guard.cfg.announce_bypass_status
            && let Some(np) = host.player(event.player).and_then(|p| snap(&*host, p))
        {
            let p = st.player(&np);
            if st.guard.is_admin(&p) {
                let on = st.guard.bypasses(&p);
                drop(st);
                tell(
                    host,
                    event.player,
                    if on {
                        "&5You are bypassing region protection. You can toggle this with /rg bypass"
                    } else {
                        "&5You are not bypassing region protection. You can toggle this with /rg bypass"
                    },
                );
            }
        }
    }

    fn player_logged_out(&self, _host: &mut dyn ServerHost, event: &mut PlayerLoggedOutEvent<'_>) {
        let mut st = lock(&self.st);
        st.tracker.forget(event.player);
        st.god.remove(&event.player);
        st.vitals.forget(event.player);
        st.bypass.remove(&event.player);
        st.sessions.forget(event.player);
    }

    // -- blocks ----------------------------------------------------------------------------

    fn break_block(&self, host: &mut dyn ServerHost, event: &mut BreakBlockEvent<'_>) -> Flow {
        let Some(ap) = host.players().acting_player() else {
            return Flow::Continue;
        };
        let Some(np) = snap(&*host, ap) else {
            return Flow::Continue;
        };
        let st = lock(&self.st);
        let p = st.player(&np);
        match st.guard.break_block(&p, bp(event.pos)) {
            Ok(()) => {
                // `exp-drops`: the block part's `exp_drop` hook is per block, so the global `break_block`
                // sample override stands in for it.
                if !st.guard.exp_drops(&p.dimension, bp(event.pos)) {
                    *event.exp_to_drop = Some(0);
                }
            }
            Err(d) => {
                drop(st);
                return Self::denied(host, &p, &d.message);
            }
        }
        drop(st);
        if self.chest_locked(host, &p, event.pos) {
            return Self::denied(host, &p, "&4This chest is protected.");
        }
        // The blacklist: `on-break` for the block, `on-destroy-with` for the tool in hand.
        let block = event.state.block().id(&*host).to_string();
        if !self.blacklist_check(host, &p, EventType::Break, &block, false) {
            return Flow::Cancel;
        }
        let tool = host
            .living(ap.entity())
            .and_then(|l| l.hand_item(&*host, Hand::Main))
            .map(|s| s.item().id(&*host).to_string())
            .unwrap_or_default();
        if !self.blacklist_check(host, &p, EventType::DestroyWith, &tool, false) {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    /// `right_click_block` also carries the item the click holds, so the wand and the item rule
    /// (horizon-rust's `use_item_on`, which arismoko answers differently) are checked here.
    fn right_click_block(&self, host: HostSide, event: &mut RightClickBlockEvent<'_>) -> Flow {
        let HostSide::Server(host) = host else {
            return Flow::Continue;
        };
        let click = event.click;
        let Some(np) = host.player(click.player).and_then(|p| snap(&*host, p)) else {
            return Flow::Continue;
        };
        let dimension = dim_id(&click.dimension);
        let block = host
            .level(&dimension)
            .and_then(|l| l.block(&*host, click.pos))
            .map(|k| k.id(&*host).to_string())
            .unwrap_or_default();
        let item = click
            .stack
            .map(|s| s.item().id(&*host).to_string())
            .unwrap_or_default();
        let st = lock(&self.st);
        let p = st.player(&np);
        // The region wand: operators see what is here.
        if item == st.guard.cfg.wand && st.guard.is_admin(&p) {
            let ids = st
                .guard
                .world(&p.dimension)
                .map(|w| wg::query::Query::new(w).ids_at(bp(click.pos)))
                .unwrap_or_default();
            drop(st);
            let msg = if ids.is_empty() {
                "&eNo regions here.".to_owned()
            } else {
                format!("&eRegions here: &7{}", ids.join(", "))
            };
            tell(host, p.subject.uuid, &msg);
            return Flow::Cancel;
        }
        if let Err(d) = st.guard.use_block(&p, bp(click.pos), &block) {
            drop(st);
            return Self::denied(host, &p, &d.message);
        }
        // A block item is checked by `entity_place`, where it would place.
        if let Err(d) = st
            .guard
            .use_item_on(&p, bp(click.pos), click.hit.face, &item, &block)
        {
            drop(st);
            return Self::denied(host, &p, &d.message);
        }
        drop(st);
        if self.chest_locked(host, &p, click.pos) {
            return Self::denied(host, &p, "&4This chest is protected.");
        }
        // The blacklist: `on-interact` for the block clicked, `on-use` for the item in hand.
        if !self.blacklist_check(host, &p, EventType::Interact, &block, false)
            || !self.blacklist_check(host, &p, EventType::Use, &item, false)
        {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    /// Before a block item places (`entity_place`): a cancel places nothing and uses nothing up.
    fn entity_place(&self, host: &mut dyn ServerHost, event: &mut EntityPlaceEvent) -> Flow {
        let Some(placer) = event.placer else {
            return Flow::Continue;
        };
        let Some(uuid) = host
            .players()
            .server_players()
            .into_iter()
            .find(|p| p.entity() == placer)
            .map(|p| p.uuid())
        else {
            // A mob's placement (a snow golem's trail, an enderman's block) is not a player's.
            return Flow::Continue;
        };
        let pos = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if matches!(self.player_check(host, uuid, |g, p| g.place_block(p, pos)), Flow::Cancel) {
            return Flow::Cancel;
        }
        let Some(p) = lock(&self.st).cached(uuid) else {
            return Flow::Continue;
        };
        let at = [event.pos.x, event.pos.y, event.pos.z];
        let block = event.state.block().id(&*host).to_string();
        let short = block.strip_prefix("minecraft:").unwrap_or(&block);
        // `chest-protection`: no chest on a spot a lock sign claims.
        if wg::chest::is_chest(&block) && self.chest_locked(host, &p, at) {
            return Self::denied(host, &p, "&4This spot is for a chest that you don't have permission for.");
        }
        if !self.blacklist_check(host, &p, EventType::Place, &block, false) {
            return Flow::Cancel;
        }
        // `simulation.sponge`: the water around a new sponge goes at the next tick.
        if short == "sponge" {
            let mut st = lock(&self.st);
            if st.guard.sponge_enabled(&p.dimension) {
                st.sponge_clears.push((p.dimension.clone(), at));
            }
        }
        Flow::Continue
    }

    fn bucket_empty(&self, host: &mut dyn ServerHost, event: &mut BucketEmptyEvent<'_>) -> Flow {
        let (uuid, pos) = (event.player, event.pos);
        self.player_check(host, uuid, |g, p| g.bucket(p, bp(pos)))
    }

    fn fill_bucket(&self, host: &mut dyn ServerHost, event: &mut FillBucketEvent<'_>) -> Flow {
        let Some(uuid) = host.players().acting_player().map(|p| p.uuid()) else {
            return Flow::Continue;
        };
        let pos = event.pos;
        self.player_check(host, uuid, |g, p| g.bucket_fill(p, bp(pos)))
    }

    fn hanging_place(&self, host: &mut dyn ServerHost, event: &mut HangingPlaceEvent<'_>) -> Flow {
        let (uuid, pos) = (event.player, event.pos);
        self.player_check(host, uuid, |g, p| g.hanging_place(p, bp(pos)))
    }

    fn hanging_break(&self, host: &mut dyn ServerHost, event: &mut HangingBreakEvent<'_>) -> Flow {
        let (kind, pos) = (event.kind, event.pos);
        match event.player {
            Some(u) => self.player_check(host, u, |g, p| {
                g.hanging_break(Some(p), &p.dimension.clone(), bp(pos), kind)
            }),
            None => {
                let st = lock(&self.st);
                if st
                    .guard
                    .hanging_break(None, event.dimension, bp(pos), kind)
                    .is_ok()
                {
                    Flow::Continue
                } else {
                    Flow::Cancel
                }
            }
        }
    }

    fn tnt_primed(&self, host: &mut dyn ServerHost, event: &mut TntPrimedEvent) -> Flow {
        let pos = event.pos;
        match event.player {
            Some(u) => self.player_check(host, u, |g, p| {
                g.tnt_prime(Some(p), &p.dimension.clone(), bp(pos))
            }),
            None => {
                // No dimension in the event: the overworld's regions decide.
                let st = lock(&self.st);
                if st.guard.tnt_prime(None, OVERWORLD, bp(pos)).is_ok() {
                    Flow::Continue
                } else {
                    Flow::Cancel
                }
            }
        }
    }

    fn explosion_detonate(
        &self,
        host: &mut dyn ServerHost,
        event: &mut ExplosionDetonateEvent<'_>,
    ) {
        let st = lock(&self.st);
        // The blast's source is not in the event: take the nearest tracked explosive.
        // TODO(arismoko-api: explosion_source): the source entity is not given, so the nearest
        // tracked explosive within 3 blocks names the blast.
        let mut best: Option<(f64, String)> = None;
        for (id, (kind, dim)) in &st.explosives {
            if dim != event.dimension {
                continue;
            }
            let Some(pos) = host.entity(*id).and_then(|e| e.position(&*host)) else {
                continue;
            };
            let d = (0..3)
                .map(|i| (pos[i] - event.center[i]).powi(2))
                .sum::<f64>();
            if d <= 9.0 && best.as_ref().is_none_or(|(b, _)| d < *b) {
                best = Some((d, kind.clone()));
            }
        }
        let blast = Blast::from_kind(best.as_ref().map(|(_, k)| k.as_str()));
        if blast == Blast::WindCharge {
            // `wind-charge-burst`: what stands in a region that denies it is neither hurt nor pushed.
            let dimension = event.dimension;
            event.entities.retain(|e| {
                e.position(&*host)
                    .is_none_or(|pos| st.guard.wind_burst_pushes(dimension, BlockPos::of(pos)))
            });
        }
        st.guard
            .explosion(event.dimension, event.center, blast, &mut *event.blocks);
        // `chest-protection`: a blast does not break a locked chest (or its sign).
        if st.guard.rule(event.dimension, "chest-protection.enable") {
            let reader = chest_world::HostReader { host: &*host, dimension: event.dimension };
            event.blocks.retain(|b| !wg::chest::is_protected(&reader, *b, None));
        }
    }

    fn piston_pre(&self, _level: &dyn LevelHost, event: &mut PistonPreEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let dim = dim_id(&event.dimension);
        let push: Vec<BlockPos> = event.push.iter().map(|p| bp_of([p.x, p.y, p.z])).collect();
        let destroy: Vec<BlockPos> = event
            .destroy
            .iter()
            .map(|p| bp_of([p.x, p.y, p.z]))
            .collect();
        if st.guard.piston(
            &dim,
            bp_of([event.piston.x, event.piston.y, event.piston.z]),
            event.direction,
            &push,
            &destroy,
        ) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    fn block_spread(&self, level: &dyn LevelHost, event: &mut BlockSpreadEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let dim = dim_id(&event.dimension);
        let (from, to) = (
            bp_of([event.from.x, event.from.y, event.from.z]),
            bp_of([event.to.x, event.to.y, event.to.z]),
        );
        let to_at = [event.to.x, event.to.y, event.to.z];
        let ok = if event.kind == "fire" {
            // `fire.disable-fire-spread-blocks`: not onto a spot with one of those below or beside it.
            let around = || {
                [[0, -1, 0], [1, 0, 0], [-1, 0, 0], [0, 0, -1], [0, 0, 1]]
                    .iter()
                    .map(|d| level_block(level, [to_at[0] + d[0], to_at[1] + d[1], to_at[2] + d[2]]))
                    .collect::<Vec<_>>()
            };
            st.guard.fire_spread(&dim, to) && !st.guard.fire_blocked_by(&dim, &around())
        } else {
            let from_block = level_block(level, [event.from.x, event.from.y, event.from.z]);
            let to_block = level_block(level, to_at);
            let below = || level_block(level, [to_at[0], to_at[1] - 1, to_at[2]]);
            let f = from_block.strip_prefix("minecraft:").unwrap_or(&from_block);
            // The fluid is told from the block it flows out of: `water-flow` and `lava-flow` apply
            // to their own fluid (both when the source block is neither).
            let flag_ok = match f {
                "water" | "bubble_column" => st.guard.fluid_flow(&dim, "water", from, to),
                "lava" => st.guard.fluid_flow(&dim, "lava", from, to),
                _ => st.guard.fluid_flow(&dim, "water", from, to) && st.guard.fluid_flow(&dim, "lava", from, to),
            };
            flag_ok && st.guard.fluid_into(&dim, &from_block, &to_block, &below) && {
                // `simulation.sponge`: water does not flow into the reach of a sponge.
                !(matches!(f, "water" | "bubble_column")
                    && st.guard.sponge_enabled(&dim)
                    && {
                        let r = st.guard.sponge_radius(&dim);
                        let redstone = st.guard.sponge_redstone(&dim);
                        // gap(has_neighbor_signal): the block_spread hook gets a LevelHost, which has no signal reads
                        // (LevelTicksHost does, for the tick hooks). Until a level handle reaches this hook, no sponge
                        // reads as powered, so a redstone-gated sponge absorbs like a plain one.
                        let powered = |_at: [i32; 3]| false;
                        let mut found = false;
                        'scan: for dx in -r..=r {
                            for dy in -r..=r {
                                for dz in -r..=r {
                                    let at = [to_at[0] + dx, to_at[1] + dy, to_at[2] + dz];
                                    if level_block(level, at) == "minecraft:sponge"
                                        && (!redstone || !powered(at))
                                    {
                                        found = true;
                                        break 'scan;
                                    }
                                }
                            }
                        }
                        found
                    })
            }
        };
        if ok { Flow::Continue } else { Flow::Cancel }
    }

    fn block_ignite(&self, level: &dyn LevelHost, event: &mut BlockIgniteEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let dim = dim_id(&event.dimension);
        let p = event.player.and_then(|u| st.cached(u));
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if p.is_none() && event.cause.to_ascii_lowercase().contains("spread") {
            let (x, y, z) = (event.pos.x, event.pos.y, event.pos.z);
            let around: Vec<String> = [[0, -1, 0], [1, 0, 0], [-1, 0, 0], [0, 0, -1], [0, 0, 1]]
                .iter()
                .map(|d| level_block(level, [x + d[0], y + d[1], z + d[2]]))
                .collect();
            if st.guard.fire_blocked_by(&dim, &around) {
                return Flow::Cancel;
            }
        }
        match st.guard.ignite(p.as_ref(), &dim, at, event.cause) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                if let Some(p) = p {
                    let msg = d.message.clone();
                    drop(st);
                    lock(&self.st).tells.push((p.subject.uuid, msg));
                }
                Flow::Cancel
            }
        }
    }

    fn block_burn(&self, level: &dyn LevelHost, event: &mut BlockBurnEvent) -> Flow {
        let st = lock(&self.st);
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        let dim = dim_id(&event.dimension);
        let block = level_block(level, [event.pos.x, event.pos.y, event.pos.z]);
        if st.guard.burn(&dim, at) && st.guard.burnable(&dim, &block) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `ice-melt`, `snow-melt`, `soil-dry`, `coral-fade`, `copper-fade` and their world options.
    fn block_fade(&self, level: &dyn LevelHost, event: &mut BlockFadeEvent) -> Flow {
        let from = state_name(level, event.from);
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if lock(&self.st).guard.block_fade(&dim_id(&event.dimension), at, &from) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `ice-form`, `snow-fall`, `frosted-ice-form`, `dynamics.snow-fall-blocks` and the world options.
    fn block_form(&self, level: &dyn LevelHost, event: &mut BlockFormEvent) -> Flow {
        let to = state_name(level, event.to);
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        let below = || level_block(level, [event.pos.x, event.pos.y - 1, event.pos.z]);
        if lock(&self.st).guard.block_form(&dim_id(&event.dimension), at, &to, &below) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `mushroom-growth` (and `dynamics.disable-mushroom-spread`) for a huge mushroom growing.
    fn block_grow_feature(&self, _level: &mut dyn LevelRandomHost, event: &mut BlockGrowFeatureEvent<'_>) -> Flow {
        let Some(feature) = event.feature.as_deref() else {
            return Flow::Continue;
        };
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if lock(&self.st).guard.block_grow(&dim_id(&event.dimension), at, feature) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `vine-growth` and `sculk-growth` (and `dynamics.disable-vine-growth`, `disable-sculk-growth`): a vine or sculk
    /// spreading into `to`. Grass, mycelium and rock growth have no hook yet.
    fn natural_block_spread(&self, _level: &dyn LevelHost, event: &mut NaturalBlockSpreadEvent<'_>) -> Flow {
        let block = match event.kind {
            "vine" => "minecraft:vine",
            "sculk" => "minecraft:sculk",
            _ => return Flow::Continue,
        };
        let at = bp_of([event.to.x, event.to.y, event.to.z]);
        if lock(&self.st).guard.block_grow(&dim_id(&event.dimension), at, block) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `sculk-growth`: a sculk block growing in place (a shrieker or sensor above a charged block).
    fn block_grow(&self, level: &dyn LevelHost, event: &mut BlockGrowEvent) -> Flow {
        let block = state_name(level, event.to);
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if lock(&self.st).guard.block_grow(&dim_id(&event.dimension), at, &block) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `potion-splash`: a thrown potion about to apply its effects. WorldGuard asks the flag at each affected
    /// entity; the hook names the landing spot only, so the flag is read there. The hook names no dimension: the
    /// thrower's decides, else the potion is kept only when a world without the flag could hold it.
    fn potion_splash_cancel(&self, host: &mut dyn ServerHost, event: &mut PotionSplashCancelEvent<'_>) -> Flow {
        let at = BlockPos::of(event.pos);
        let dimension = acting_dimension(&*host);
        let st = lock(&self.st);
        let allowed = match dimension {
            Some(d) => st.guard.cloud_spawns(&d, at),
            None => {
                let dims = region_dimensions(&st.guard);
                dims.is_empty() || dims.iter().any(|d| st.guard.cloud_spawns(d, at))
            }
        };
        if allowed { Flow::Continue } else { Flow::Cancel }
    }

    /// `mobs.block-zombie-door-destruction`.
    fn entity_break_door_cancel(&self, host: &mut dyn ServerHost, event: &mut EntityBreakDoorCancelEvent<'_>) -> Flow {
        let dimension = host
            .entity(event.entity)
            .and_then(|e| e.level(&*host))
            .map(|l| l.dimension_id().to_string())
            .unwrap_or_else(|| OVERWORLD.to_owned());
        if lock(&self.st).guard.door_break_blocked(&dimension) { Flow::Cancel } else { Flow::Continue }
    }

    /// `player-damage.disable-death-messages`: nothing is said when a player dies.
    fn player_death_message(&self, host: &mut dyn ServerHost, event: &mut PlayerDeathMessageEvent<'_>) {
        let dimension = host
            .player(event.player)
            .and_then(|p| p.level(&*host))
            .map(|l| l.dimension_id().to_string())
            .unwrap_or_else(|| OVERWORLD.to_owned());
        if lock(&self.st).guard.rule(&dimension, "player-damage.disable-death-messages") {
            *event.message = None;
        }
    }

    /// The blacklist's `on-dispense`.
    fn block_dispense(&self, host: &mut dyn ServerHost, event: &mut BlockDispenseEvent<'_>) -> Flow {
        let item = event.stack.item().id(&*host).to_string();
        if self.dispense_allowed(event.dimension, &item) { Flow::Continue } else { Flow::Cancel }
    }

    /// `weather.disable-weather`, `disable-thunderstorm`, `always-raining`, `always-thundering`.
    fn weather_change(&self, _host: &mut dyn ServerHost, event: &mut WeatherChangeEvent<'_>) -> Flow {
        let mut st = lock(&self.st);
        let now = (event.raining, event.thundering);
        let was = st.weather.get(event.dimension).copied().unwrap_or((false, false));
        if st.guard.weather_change(event.dimension, was, now) {
            st.weather.insert(event.dimension.to_owned(), now);
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `weather.disable-pig-zombification`, `disable-villager-witchification`, `disable-powered-creepers`.
    /// A cancel also spares the animal the bolt's damage, which WorldGuard's own events do not.
    fn entity_struck_by_lightning(&self, host: &mut dyn ServerHost, event: &mut EntityStruckByLightningEvent) -> Flow {
        let Some(kind) = event.entity.kind(&*host).map(|k| k.id(&*host).to_string()) else {
            return Flow::Continue;
        };
        let dimension = event
            .entity
            .level(&*host)
            .map(|l| l.dimension_id().to_string())
            .unwrap_or_else(|| OVERWORLD.to_owned());
        if lock(&self.st).guard.zap_allowed(&dimension, &kind) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `chest-protection` and `chest-protection.disable-off-check`: a `[Lock]` sign is checked when it is written.
    fn sign_changed(&self, host: &mut dyn ServerHost, event: &mut SignChangedEvent<'_>) -> Flow {
        if !event.front || !event.lines.first().is_some_and(|l| l.eq_ignore_ascii_case("[Lock]")) {
            return Flow::Continue;
        }
        let (enabled, off_check_disabled) = {
            let st = lock(&self.st);
            (
                st.guard.rule(event.dimension, "chest-protection.enable"),
                st.guard.rule(event.dimension, "chest-protection.disable-off-check"),
            )
        };
        let uuid = event.player;
        let (x, y, z) = (event.pos[0], event.pos[1], event.pos[2]);
        let outcome: Result<&'static str, &'static str> = if enabled {
            let overridden = may(host, uuid, "worldguard.chest-protection.override", 0)
                || may(host, uuid, "worldguard.override.chest-protection", 0);
            let reader = chest_world::HostReader { host: &*host, dimension: event.dimension };
            let protected_here =
                !overridden && wg::chest::is_protected_placement(&reader, event.pos, Some(event.name));
            wg::chest::check_lock_sign(&reader, event.pos, event.name, event.lines, protected_here)
        } else if !off_check_disabled {
            Err("WorldGuard's sign chest protection is disabled.")
        } else {
            return Flow::Continue;
        };
        match outcome {
            Ok(msg) => {
                event.lines[0] = "[Lock]".to_owned();
                tell(host, uuid, &format!("&e{msg}"));
                Flow::Continue
            }
            Err(msg) => {
                tell(host, uuid, &format!("&c{msg}"));
                // `breakNaturally`: the sign goes and drops itself.
                lock(&self.st)
                    .commands
                    .push(format!("execute in {} run setblock {x} {y} {z} minecraft:air destroy", event.dimension));
                Flow::Cancel
            }
        }
    }

    fn leaves_decay(&self, _level: &dyn LevelHost, event: &mut LeavesDecayEvent) -> Flow {
        let st = lock(&self.st);
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if st.guard.leaf_decay(&dim_id(&event.dimension), at) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    // -- flags that name one event ---------------------------------------------------------

    /// `block-trampling`: a landing about to turn farmland to dirt or break an egg.
    fn entity_change_block_pre(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityChangeBlockPreEvent<'_>,
    ) -> Flow {
        let block = event.from.block().id(&*host).to_string();
        if !matches!(
            block.strip_prefix("minecraft:").unwrap_or(&block),
            "farmland" | "turtle_egg" | "sniffer_egg"
        ) {
            return Flow::Continue;
        }
        let pos = bp(event.pos);
        if let Some(uuid) = event.player {
            let Some(np) = host.player(uuid).and_then(|p| snap(&*host, p)) else {
                return Flow::Continue;
            };
            let st = lock(&self.st);
            let p = st.player(&np);
            return if st.guard.trample_by_player(&p, pos, &block).is_ok() {
                Flow::Continue
            } else {
                Flow::Cancel
            };
        }
        let dimension = event
            .entity
            .and_then(|id| host.entity(id))
            .and_then(|e| e.level(&*host))
            .map(|l| l.dimension_id().to_string())
            .unwrap_or_else(|| OVERWORLD.to_owned());
        if lock(&self.st).guard.trample_by_mob(&dimension, pos, &block) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `enderman-grief`, `snowman-trails`, `ravager-grief` (and the two world options): the mob is
    /// told it may not change blocks. The block it would change is not given, so the mob's own
    /// position decides.
    fn entity_mob_griefing(&self, host: &dyn Host, event: &mut EntityMobGriefingEvent) {
        if !event.can_grief {
            return;
        }
        let Some(kind) = event.entity.kind(host).map(|k| k.id(host).to_string()) else {
            return;
        };
        if !matches!(
            kind.strip_prefix("minecraft:").unwrap_or(&kind),
            "enderman" | "snow_golem" | "ravager"
        ) {
            return;
        }
        let Some(pos) = event.entity.position(host) else {
            return;
        };
        let dimension = event
            .entity
            .level(host)
            .map(|l| l.dimension_id().to_string())
            .unwrap_or_else(|| OVERWORLD.to_owned());
        if !lock(&self.st).guard.mob_may_grief(&dimension, &kind, BlockPos::of(pos)) {
            event.can_grief = false;
        }
    }

    /// `crop-growth`.
    fn crop_grow_pre(&self, _level: &mut dyn LevelRandomHost, event: &mut CropGrowPreEvent<'_>) {
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if !lock(&self.st).guard.crop_grows(&dim_id(&event.dimension), at) {
            *event.result = TriState::False;
        }
    }

    /// `moisture-change`.
    fn moisture_change(&self, _level: &dyn LevelHost, event: &mut MoistureChangeEvent) -> Flow {
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if lock(&self.st).guard.moisture_changes(&dim_id(&event.dimension), at) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `lava-harden`: any fluid placing a block (cobblestone, stone, basalt, obsidian) counts.
    fn fluid_place_block(&self, _level: &dyn LevelHost, event: &mut FluidPlaceBlockEvent<'_>) -> Flow {
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if lock(&self.st).guard.lava_hardens(&dim_id(&event.dimension), at) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }


    /// `exp-drops` for the experience of a dying mob or player.
    fn living_experience_drop(
        &self,
        host: &mut dyn ServerHost,
        event: &mut LivingExperienceDropEvent<'_>,
    ) -> Flow {
        let Some(entity) = host.entity(event.entity) else {
            return Flow::Continue;
        };
        let Some(pos) = entity.position(&*host) else {
            return Flow::Continue;
        };
        let dimension = entity
            .level(&*host)
            .map(|l| l.dimension_id().to_string())
            .unwrap_or_else(|| OVERWORLD.to_owned());
        if lock(&self.st).guard.exp_drops(&dimension, BlockPos::of(pos)) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `natural-health-regen`. The event does not say why the entity heals; natural regeneration
    /// heals at most one point at a time, so only heals of one point or less are held back
    /// (HMOD-GAP(heal_cause): a Regeneration effect is held back too).
    fn living_heal(&self, _context: &dyn LoopContext, event: &mut LivingHealEvent) -> Flow {
        if event.amount > 1.0 {
            return Flow::Continue;
        }
        let st = lock(&self.st);
        let Some(p) = st.by_entity.get(&(event.entity as i32)).map(|s| st.player(s)) else {
            return Flow::Continue;
        };
        if st.guard.natural_regen(&p) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `natural-hunger-drain`: only a drop of the food level (eating raises it).
    fn food_level_changed(
        &self,
        _host: &mut dyn ServerHost,
        event: &mut FoodLevelChangedEvent<'_>,
    ) -> Flow {
        if event.to > event.from {
            return Flow::Continue;
        }
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else {
            return Flow::Continue;
        };
        if st.guard.natural_hunger(&p) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    /// `/halt-activity`: no explosions.
    fn explosion_start(&self, _host: &mut dyn ServerHost, _event: &mut ExplosionStartEvent<'_>) -> Flow {
        if lock(&self.st).guard.activity_halted {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }

    // -- entities --------------------------------------------------------------------------

    fn attack_entity(&self, host: &mut dyn ServerHost, event: &mut AttackEntityEvent<'_>) -> Flow {
        let Some(ap) = host.players().acting_player() else {
            return Flow::Continue;
        };
        let Some(np) = snap(&*host, ap) else {
            return Flow::Continue;
        };
        let st = lock(&self.st);
        let p = st.player(&np);
        let victim = if event.kind == "minecraft:player" {
            st.by_entity.get(&event.entity).map(|v| st.player(v))
        } else {
            None
        };
        match st
            .guard
            .attack_entity(&p, event.kind, event.pos, victim.as_ref())
        {
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
            }
        }
    }

    // A cancel is answered as `InteractionResult::Fail` (the hook's own refusal; the old `Flow::Cancel`).
    fn entity_interact(&self, host: HostSide, event: &mut EntityInteractEvent<'_>) -> Option<InteractionResult> {
        let HostSide::Server(host) = host else {
            return None;
        };
        let Some(ap) = host.players().acting_player() else {
            return None;
        };
        let Some(np) = snap(&*host, ap) else {
            return None;
        };
        let at = host
            .entity(event.entity as u32)
            .and_then(|e| e.position(&*host))
            .unwrap_or(np.position);
        let st = lock(&self.st);
        let p = st.player(&np);
        match st.guard.interact_entity(&p, event.kind, at) {
            Ok(()) => None,
            Err(d) => {
                drop(st);
                let _ = Self::denied(host, &p, &d.message);
                Some(InteractionResult::Fail)
            }
        }
    }

    fn armor_stand_manipulate(
        &self,
        host: &mut dyn ServerHost,
        event: &mut ArmorStandManipulateEvent<'_>,
    ) -> Flow {
        let at = host
            .entity(event.stand_id as u32)
            .and_then(|e| e.position(&*host));
        let uuid = event.player;
        self.player_check(host, uuid, |g, p| {
            g.interact_entity(p, "minecraft:armor_stand", at.unwrap_or(p.pos))
        })
    }

    fn entity_mount(&self, host: &mut dyn ServerHost, event: &mut EntityMountEvent<'_>) -> Flow {
        if event.mounting
            && event.kind != "minecraft:player"
            && wg::kinds::is_vehicle(event.vehicle_kind)
            && lock(&self.st).guard.vehicle_entry_blocked(event.dimension)
        {
            // `mobs.block-vehicle-entry`
            return Flow::Cancel;
        }
        if !event.mounting || event.kind != "minecraft:player" {
            return Flow::Continue;
        }
        let at = host.entity(event.vehicle).and_then(|e| e.position(&*host));
        let uuid = {
            let st = lock(&self.st);
            st.by_entity.get(&(event.entity as i32)).map(|p| p.uuid)
        };
        let Some(uuid) = uuid else {
            return Flow::Continue;
        };
        let vk = event.vehicle_kind.to_owned();
        self.player_check(host, uuid, |g, p| g.mount(p, &vk, at.unwrap_or(p.pos)))
    }

    fn living_incoming_damage(
        &self,
        _level: &mut dyn LevelRandomHost,
        event: &mut LivingIncomingDamageEvent<'_>,
    ) -> Flow {
        if event.kind == "minecraft:armor_stand" {
            // `mobs.block-armor-stand-destroy`: what is not a player's blow. The event names no
            // dimension, so the overworld's option decides.
            let st = lock(&self.st);
            let by_player = st.by_entity.contains_key(&event.source.causing);
            return if !by_player && st.guard.armor_stand_hit_blocked(OVERWORLD) {
                Flow::Cancel
            } else {
                Flow::Continue
            };
        }
        if event.kind != "minecraft:player" {
            return Flow::Continue;
        }
        let st = lock(&self.st);
        if st
            .by_entity
            .get(&event.entity)
            .is_some_and(|v| st.god.contains(&v.uuid))
        {
            return Flow::Cancel;
        }
        let Some(victim) = st.by_entity.get(&event.entity).map(|v| st.player(v)) else {
            return Flow::Continue;
        };
        if st.guard.damage_rescues(&victim, event.source.damage_type) {
            // `teleport-on-void-falling` / `teleport-on-suffocation`: no damage, and the next tick moves the player.
            let uuid = victim.subject.uuid;
            drop(st);
            lock(&self.st).rescues.push(uuid);
            return Flow::Cancel;
        }
        let causing = event.source.causing;
        let attacker = st.by_entity.get(&causing).map(|a| st.player(a));
        let by_mob = causing != 0 && attacker.is_none() && causing != event.entity;
        if st
            .guard
            .damage_player(&victim, event.source.damage_type, attacker.as_ref(), by_mob)
        {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    fn entity_join_level(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityJoinLevelEvent<'_>,
    ) -> Flow {
        let mut st = lock(&self.st);
        let k = event.kind.strip_prefix("minecraft:").unwrap_or(event.kind);
        if k == "lightning_bolt" {
            // `weather.prevent-lightning-strike-blocks` and the `lightning` flag.
            let at = [event.pos[0].floor() as i32, event.pos[1].floor() as i32, event.pos[2].floor() as i32];
            let block = host_block(&*host, event.dimension, at);
            let below = host_block(&*host, event.dimension, [at[0], at[1] - 1, at[2]]);
            if !st.guard.lightning_strikes(event.dimension, bp(at), &block, &below) {
                return Flow::Cancel;
            }
            return Flow::Continue;
        }
        if matches!(
            k,
            "tnt"
                | "tnt_minecart"
                | "creeper"
                | "fireball"
                | "small_fireball"
                | "dragon_fireball"
                | "wither_skull"
                | "wither"
                | "ender_dragon"
                | "end_crystal"
                | "breeze_wind_charge"
                | "wind_charge"
        ) {
            st.explosives.insert(
                event.entity,
                (event.kind.to_owned(), event.dimension.to_owned()),
            );
        }
        if event.loaded_from_disk {
            return Flow::Continue;
        }
        // `potion-splash` also covers the lingering clouds (and dragon breath).
        if k == "area_effect_cloud" && !st.guard.cloud_spawns(event.dimension, BlockPos::of(event.pos)) {
            return Flow::Cancel;
        }
        if st.guard.mob_spawn(event.dimension, event.kind, event.pos) {
            Flow::Continue
        } else {
            st.explosives.remove(&event.entity);
            Flow::Cancel
        }
    }

    fn entity_leave_level(
        &self,
        _host: &mut dyn ServerHost,
        event: &mut EntityLeaveLevelEvent<'_>,
    ) {
        lock(&self.st).explosives.remove(&event.entity);
    }

    // -- players ---------------------------------------------------------------------------

    fn item_picked_up(
        &self,
        host: &mut dyn ServerHost,
        event: &mut ItemPickedUpEvent<'_>,
    ) {
        // Upstream made this a listener with `can_pickup` (NeoForge's ItemEntityPickupEvent.Pre): False leaves it.
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else {
            return;
        };
        if st.guard.item_pickup(&p).is_err() {
            *event.can_pickup = TriState::False;
            return;
        }
        drop(st);
        // `on-acquire` of the blacklist, silent as WorldGuard asks it for a pickup.
        let item = event
            .item
            .stack(&*host)
            .map(|s| s.item().id(&*host).to_string())
            .unwrap_or_default();
        if !self.blacklist_check(host, &p, EventType::Acquire, &item, true) {
            *event.can_pickup = TriState::False;
        }
    }

    /// `item-drop`: a cancel puts the stack back.
    fn item_toss(&self, host: &mut dyn ServerHost, event: &mut ItemTossEvent) -> Flow {
        let uuid = event.player;
        if matches!(self.player_check(host, uuid, |g, p| g.item_drop(p)), Flow::Cancel) {
            return Flow::Cancel;
        }
        let item = event
            .item
            .stack(&*host)
            .map(|s| s.item().id(&*host).to_string())
            .unwrap_or_default();
        let Some(p) = lock(&self.st).cached(uuid) else {
            return Flow::Continue;
        };
        if self.blacklist_check(host, &p, EventType::Drop, &item, false) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    fn ender_pearl_teleport(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EnderPearlTeleportEvent<'_>,
    ) -> Flow {
        let owner = event.owner;
        let Some(uuid) = host
            .players()
            .server_players()
            .into_iter()
            .find(|p| p.entity() == owner)
            .map(|p| p.uuid())
        else {
            return Flow::Continue;
        };
        let target = *event.target;
        let f = self.player_check(host, uuid, |g, p| g.ender_pearl(p, target));
        if f == Flow::Continue {
            lock(&self.st).teleported.insert(uuid);
        }
        f
    }

    /// `entry` / `exit` for a step before it is applied (`movement::check_step`): a refused step is
    /// cancelled, the player stays at `from`. Chorus fruit is not guarded: its hook
    /// (`item_consumption_teleport`) gives a `LivingRef`, which has no player lookup here.
    fn player_move(&self, host: &mut dyn ServerHost, event: &mut PlayerMoveEvent) -> Flow {
        let Some(ap) = host.player(event.player) else {
            return Flow::Continue;
        };
        let Some(s) = snap(&*host, ap) else {
            return Flow::Continue;
        };
        let st = lock(&self.st);
        let p = st.player(&s);
        let no_fill = |m: &str, _id: &str| m.to_owned();
        let Some(msg) = wg::movement::check_step(&st.guard, &p, event.from, &no_fill) else {
            return Flow::Continue;
        };
        drop(st);
        Self::denied(host, &p, &msg)
    }

    fn player_teleported(&self, _host: &mut dyn ServerHost, event: &mut PlayerTeleportedEvent<'_>) {
        lock(&self.st).teleported.insert(event.player);
    }

    fn can_player_sleep(&self, _host: &mut dyn ServerHost, event: &mut CanPlayerSleepEvent<'_>) {
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else {
            return;
        };
        if let Err(d) = st.guard.sleep(&p, bp(event.pos)) {
            *event.problem = Some(text::plain(&d.message));
        }
    }

    fn server_chat(&self, _host: &mut dyn ServerHost, event: &mut ServerChatEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else {
            return Flow::Continue;
        };
        match st.guard.send_chat(&p) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                if !d.message.is_empty() {
                    event.replies.push(line(&d.message));
                }
                Flow::Cancel
            }
        }
    }

    fn tab_complete(&self, host: &mut dyn ServerHost, event: &mut TabCompleteEvent<'_>) {
        let command: &str = event.command;
        let Some(first_space) = command.find(' ') else { return };
        let root = command[..first_space].to_ascii_lowercase();
        let rest = &command[first_space + 1..];
        let words = match root.as_str() {
            "rg" | "region" | "regions" => self.complete_region(&mut *host, event.player, rest),
            "wg" | "worldguard" => self.complete_wg(&mut *host, event.player, rest),
            _ => return,
        };
        let word_start = command.rfind(' ').map_or(0, |i| i + 1);
        let (from, to) = (event.range[0], event.range[1]);
        for w in words {
            // The engine's range may begin before the word (it took the rest of the line for one argument).
            let text = if word_start >= from { format!("{}{}", &command[from..word_start], w) } else { w };
            if !event.suggestions.iter().any(|s| s.text == text) {
                event.suggestions.push(Suggestion::new((from, to), text));
            }
        }
    }

    fn player_command(
        &self,
        _host: &mut dyn ServerHost,
        event: &mut PlayerCommandEvent<'_>,
    ) -> Flow {
        // `//god` is not a command: only once the engine tells the slash count apart (see `SLASH_AWARE`).
        let double = event.command.starts_with('/');
        if double {
            SLASH_AWARE.store(true, Ordering::Relaxed);
            let root = event.command.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
            if OUR_ROOTS.contains(&root.as_str()) {
                event.replies.push(line("&cUnknown or incomplete command, see below for error"));
                event.cancelled = true;
                return Flow::Cancel;
            }
        }
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else {
            return Flow::Continue;
        };
        match st.guard.command(&p, event.command) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                if !d.message.is_empty() {
                    event.replies.push(line(&d.message));
                }
                Flow::Cancel
            }
        }
    }
}
