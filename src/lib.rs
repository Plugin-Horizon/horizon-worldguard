//! horizon-worldguard as an arismoko native trait mod (`mod_api`). The rules are the unchanged
//! `worldguard-core` crate (`core/`, the former `api/src/wg/`); this file maps arismoko's hooks and
//! host calls onto them. Where arismoko has no API yet the line is marked
//! `TODO(arismoko-api: <name>)` and says what is lost meanwhile. PORT_NOTES.md lists them all.
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
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use wg::commands::{self, Effect, Players};
use wg::config::{Config as WgConfig, DEFAULT_TOML};
use wg::geom::BlockPos;
use wg::guard::{Blast, Guard, Player};
use wg::movement::{MoveEvent, Tracker};
use wg::region::Subject;
use wg::selection::Selections;
use wg::store::World;
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
    #[allow(dead_code)]
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

    fn load_world(&mut self, name: &str) -> String {
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
    host: &'a dyn ServerHost,
    op_level: u8,
}

impl Players for KnownPlayers<'_> {
    // A permission provider's value for the node first (LuckPerms answers through the
    // `player_permission_check` hook); where none decided, the operator level as before.
    fn has_permission(&self, uuid: u128, node: &str) -> bool {
        let player = self.host.player(uuid);
        if let Some(answer) = player.and_then(|p| p.permission_value(self.host, node)) {
            return answer;
        }
        let level = player
            .and_then(|p| p.permission_level(self.host))
            .unwrap_or(0);
        level >= self.op_level
    }
    fn resolve(&self, name: &str) -> Option<(u128, String)> {
        self.names.get(&name.to_ascii_lowercase()).cloned()
    }
    fn name_of(&self, uuid: u128) -> Option<String> {
        self.names
            .values()
            .find(|(u, _)| *u == uuid)
            .map(|(_, n)| n.clone())
    }
}

impl Mod {
    fn load_config(&self, text: &str, what: &str) {
        match WgConfig::parse(text) {
            Ok(cfg) => {
                lock(&self.st).guard.cfg = cfg;
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
            host: &*host,
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
                // The Java plugin's version (plugin.yml), not this port's crate version.
                "&dWorldGuard 7.0.20+2410-0b6c3f1".to_owned(),
                "&dhttp://www.enginehub.org".to_owned(),
                "&7----------- Platforms -----------".to_owned(),
                "&7* Horizon".to_owned(),
            ],
            "report" | "profile" | "stopprofile" | "debug" => {
                vec![format!("&c/wg {sub} is not available yet.")]
            }
            _ => vec![
                "&cUnknown subcommand. Use /wg reload, flushstates, running or version.".into(),
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
            _ => {
                if let Some(s) = sender {
                    if let Some(node) = wg_node(args) {
                        let op_level = lock(&self.st).guard.cfg.op_level;
                        if !may(&*host, s.uuid, node, op_level) {
                            return vec![NO_PERMISSION.to_owned()];
                        }
                    }
                }
                self.wg_command(host, args)
            }
        }
    }

    /// WorldGuard's general commands `/god`, `/ungod`, `/heal`, `/slay` (own or `<player>`; the
    /// `.other` node for another player). Not ported: `/stack`, `/stoplag`, `/halt-activity`,
    /// `/locate` (see PORT_NOTES.md).
    fn general_command(
        &self,
        host: &mut dyn ServerHost,
        root: &str,
        sender: Option<&Snap>,
        args: &str,
    ) -> Vec<String> {
        let base = format!("worldguard.{}", if root == "ungod" { "god" } else { root });
        let op_level = lock(&self.st).guard.cfg.op_level;
        if let Some(s) = sender {
            if !may(&*host, s.uuid, &base, op_level) {
                return vec![NO_PERMISSION.to_owned()];
            }
        }
        let (uuid, name, other) = match args.split_whitespace().next() {
            None => match sender {
                Some(s) => (s.uuid, s.name.clone(), false),
                None => return vec!["&cA player is expected.".to_owned()],
            },
            Some(n) => {
                if let Some(s) = sender {
                    if !may(&*host, s.uuid, &format!("{base}.other"), op_level) {
                        return vec![NO_PERMISSION.to_owned()];
                    }
                }
                let st = lock(&self.st);
                match st.names.get(&n.to_ascii_lowercase()).cloned() {
                    Some((u, real)) if st.by_uuid.contains_key(&u) => (u, real, true),
                    _ => return vec![format!("&cNo players matched query '{n}'.")],
                }
            }
        };
        let mut st = lock(&self.st);
        match root {
            "god" => {
                st.god.insert(uuid);
                vec![if other { format!("&dEnabled god mode for {name}.") } else { "&dGodmode enabled!".to_owned() }]
            }
            "ungod" => {
                st.god.remove(&uuid);
                vec![if other { format!("&dDisabled god mode for {name}.") } else { "&dGodmode disabled!".to_owned() }]
            }
            "heal" => {
                // The name comes from the players seen on this server, never from the typed text.
                st.commands.push(format!("effect give {name} minecraft:instant_health 1 10 true"));
                st.commands.push(format!("effect give {name} minecraft:saturation 1 20 true"));
                vec![if other { format!("&dHealed {name}.") } else { "&dHealed!".to_owned() }]
            }
            _ => {
                st.commands.push(format!("kill {name}"));
                vec![if other { format!("&dSlain {name}.") } else { "&dSlain!".to_owned() }]
            }
        }
    }
}

const NO_PERMISSION: &str = "&cYou don't have permission to do that.";

/// The node a `/wg` sub-command needs (`None`: open to everybody, the version).
fn wg_node(args: &str) -> Option<&'static str> {
    match args.split_whitespace().next().unwrap_or("").to_ascii_lowercase().as_str() {
        "" | "version" => None,
        "flushstates" | "clearstates" => Some("worldguard.flushstates"),
        "running" | "queue" => Some("worldguard.running"),
        "report" | "profile" | "stopprofile" | "debug" => Some("worldguard.report"),
        _ => Some("worldguard.reload"),
    }
}

// --- slash forms ----------------------------------------------------------------------------------
//
// WorldGuard's commands (`/god`, `/rg`, `/wg`...) exist with ONE slash only; `//god` is not a command. The
// player-command hook hears the line without its slashes, and the engine builds so far trimmed ALL of them, so the two
// look alike. An engine that trims only one slash delivers `/god` for a typed `//god`: from the first line that begins
// with `/` the double-slash forms of our roots are refused as unknown.

static SLASH_AWARE: AtomicBool = AtomicBool::new(false);

const OUR_ROOTS: &[&str] = &["rg", "region", "regions", "wg", "worldguard", "god", "ungod", "heal", "slay"];

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
fn may_rg_sub(host: &dyn ServerHost, uuid: u128, op_level: u8, player_commands: &[String], sub: &str) -> bool {
    let Some(p) = host.player(uuid) else { return false };
    if p.permission_level(host).unwrap_or(0) >= op_level || player_commands.iter().any(|c| c == sub) {
        return true;
    }
    ["", ".own", ".member"].iter().any(|tail| p.permission_value(host, &format!("worldguard.region.{sub}{tail}")) == Some(true))
}

impl Mod {
    /// The words that complete the last word of `rest` (what follows `/rg `).
    fn complete_region(&self, host: &dyn ServerHost, uuid: u128, rest: &str) -> Vec<String> {
        let tokens: Vec<&str> = rest.split(' ').collect();
        let n = tokens.len() - 1;
        let prefix = tokens[n];
        let (op_level, player_commands, world_of_player) = {
            let st = lock(&self.st);
            let dimension = host
                .player(uuid)
                .and_then(|p| p.entity().level(host))
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
    fn complete_wg(&self, host: &dyn ServerHost, uuid: u128, rest: &str) -> Vec<String> {
        if rest.contains(' ') {
            return Vec::new();
        }
        let op_level = lock(&self.st).guard.cfg.op_level;
        ["version", "reload", "flushstates", "running"]
            .iter()
            .filter(|sub| wg_node(sub).is_none_or(|node| may(host, uuid, node, op_level)))
            .filter(|sub| starts_with_any_case(sub, rest))
            .map(|sub| (*sub).to_owned())
            .collect()
    }
}

/// A provider's answer for `node`, else the operator level (the same rule as `KnownPlayers`).
fn may(host: &dyn ServerHost, uuid: u128, node: &str, op_level: u8) -> bool {
    host.player(uuid).is_some_and(|p| {
        let level = p.permission_level(host).unwrap_or(0);
        p.permission_value(host, node).unwrap_or(level >= op_level)
    })
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
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
        for root in ["wg", "worldguard", "god", "ungod", "heal", "slay"] {
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
            "rg" | "region" | "regions" | "wg" | "worldguard" | "god" | "ungod" | "heal" | "slay"
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
            let State { guard, tracker, .. } = &mut *st;
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
        let cmds = std::mem::take(&mut st.commands);
        let op_level = st.guard.cfg.op_level;
        if st.save_at.is_some_and(|t| st.ticks >= t) {
            st.save_at = None;
            st.save_dirty();
        }
        drop(st);
        for (u, dim, to, yaw, pitch) in pushes {
            teleport(host, u, &dim, to, yaw, pitch);
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
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
            }
        }
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
        match st
            .guard
            .use_item_on(&p, bp(click.pos), click.hit.face, &item, &block)
        {
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
            }
        }
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
        self.player_check(host, uuid, |g, p| g.place_block(p, pos))
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
                // No dimension in the event: the overworld's regions decide.
                let st = lock(&self.st);
                if st
                    .guard
                    .hanging_break(None, OVERWORLD, bp(pos), kind)
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
        st.guard
            .explosion(event.dimension, event.center, blast, &mut *event.blocks);
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

    fn block_spread(&self, _level: &dyn LevelHost, event: &mut BlockSpreadEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let dim = dim_id(&event.dimension);
        let (from, to) = (
            bp_of([event.from.x, event.from.y, event.from.z]),
            bp_of([event.to.x, event.to.y, event.to.z]),
        );
        let ok = if event.kind == "fire" {
            st.guard.fire_spread(&dim, to)
        } else {
            // TODO(arismoko-api: fluid_kind_in_spread): which fluid is not told, so water-flow and
            // lava-flow both apply to any fluid.
            st.guard.fluid_flow(&dim, "water", from, to)
                && st.guard.fluid_flow(&dim, "lava", from, to)
        };
        if ok { Flow::Continue } else { Flow::Cancel }
    }

    fn block_ignite(&self, _level: &dyn LevelHost, event: &mut BlockIgniteEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let dim = dim_id(&event.dimension);
        let p = event.player.and_then(|u| st.cached(u));
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
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

    fn block_burn(&self, _level: &dyn LevelHost, event: &mut BlockBurnEvent) -> Flow {
        let st = lock(&self.st);
        let at = bp_of([event.pos.x, event.pos.y, event.pos.z]);
        if st.guard.burn(&dim_id(&event.dimension), at) {
            Flow::Continue
        } else {
            Flow::Cancel
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

    fn entity_interact(
        &self,
        host: &mut dyn ServerHost,
        event: &mut EntityInteractEvent<'_>,
    ) -> Flow {
        let Some(ap) = host.players().acting_player() else {
            return Flow::Continue;
        };
        let Some(np) = snap(&*host, ap) else {
            return Flow::Continue;
        };
        let at = host
            .entity(event.entity as u32)
            .and_then(|e| e.position(&*host))
            .unwrap_or(np.position);
        let st = lock(&self.st);
        let p = st.player(&np);
        match st.guard.interact_entity(&p, event.kind, at) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
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
        _host: &mut dyn ServerHost,
        event: &mut EntityJoinLevelEvent<'_>,
    ) -> Flow {
        let mut st = lock(&self.st);
        let k = event.kind.strip_prefix("minecraft:").unwrap_or(event.kind);
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
        _host: &mut dyn ServerHost,
        event: &mut ItemPickedUpEvent<'_>,
    ) {
        // Upstream made this a listener with `can_pickup` (NeoForge's ItemEntityPickupEvent.Pre): False leaves it.
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else {
            return;
        };
        if st.guard.item_pickup(&p).is_err() {
            *event.can_pickup = TriState::False;
        }
    }

    /// `item-drop`: a cancel puts the stack back.
    fn item_toss(&self, host: &mut dyn ServerHost, event: &mut ItemTossEvent<'_>) -> Flow {
        let uuid = event.player;
        self.player_check(host, uuid, |g, p| g.item_drop(p))
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
            "rg" | "region" | "regions" => self.complete_region(&*host, event.player, rest),
            "wg" | "worldguard" => self.complete_wg(&*host, event.player, rest),
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
