//! horizon-worldguard: region protection for Horizon servers, as a trait mod
//! (docs/native-mods.md, "Trait mods").
//!
//! The rules live in `wg` (engine-free, tested on its own through `core/`). This file only maps
//! Horizon's hooks to them. It was written against Horizon a589a3dd9 without building the
//! engine: lines marked `UNVERIFIED` guess a type's shape and are the first to check when it
//! compiles (STATUS.md).

// Some rules wait for hooks Horizon does not have yet (GAPS.md); they stay, tested, in `wg`.
#[allow(dead_code)]
mod wg;

#[cfg(feature = "worldedit_server")]
mod we_selection;

use horizon_client::mod_api::{
    ArmorStandManipulateEvent, AttackEntityEvent, BlockBurnEvent, BlockIgniteEvent, BlockPlaceEvent,
    BlockSpreadEvent, BucketEmptyEvent, CanPlayerSleepEvent, ChorusFruitTeleportEvent,
    ConfigLoadingEvent, ConfigReloadingEvent, DestroyBlockEvent, EnderPearlTeleportEvent,
    EntityJoinLevelEvent, EntityLeaveLevelEvent, EntityMountEvent, ExplosionDetonateEvent,
    FillBucketEvent, Flow, HangingBreakEvent, HangingPlaceEvent, Hooks, Host, InteractEntityEvent,
    ItemDroppedV2Event, ItemPickedUpEvent, LeavesDecayEvent, LevelHost, LivingIncomingDamageEvent, NativeCommand,
    NativeConfig, NativeConfigKind, NativeServerPlayer, PerformCommandEvent,
    PerformConsoleCommandEvent, PistonMoveEvent, PlayerChatEvent, PlayerPortalEvent,
    PlayerCommandEvent, PlayerLoggedInEvent, PlayerLoggedOutEvent, PlayerMoveEvent,
    PlayerTeleportedEvent,
    RegisterCommandsEvent, RegisterConfigsEvent, RightClickBlockEvent, ServerStoppingEvent, Text,
    TickEvent, TntPrimedEvent, UseItemOnEvent,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use wg::commands::{self, Effect, Players};
use wg::config::{Config, DEFAULT_TOML};
use wg::geom::BlockPos;
use wg::guard::{Blast, Guard, Player};
use wg::movement::{MoveEvent, Tracker};
use wg::region::Subject;
use wg::selection::Selections;
use wg::store::World;
use wg::text;

const MOD_ID: &str = "worldguard";
/// Server ticks a second.
const TPS: u64 = 20;

/// The `/wg` usages the client completes, without the root name (`wg_command` answers each).
const WG_USAGES: &[&str] = &[
    "reload",
    "flushstates [player:player_name]",
    "clearstates [player:player_name]",
    "running",
    "queue",
    "version",
    "report",
    "profile",
    "stopprofile",
    "debug",
    "debug testbreak",
    "debug testplace",
    "debug testinteract",
    "debug testdamage",
];

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(WorldGuard {
        st: Mutex::new(State::new()),
    })
}

struct WorldGuard {
    st: Mutex<State>,
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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
    /// The players as of the last tick, by entity id (for hooks without a host).
    by_entity: HashMap<i32, NativeServerPlayer>,
    by_uuid: HashMap<u128, NativeServerPlayer>,
    /// Names seen this session, lower case -> (uuid, name), for /rg addmember.
    names: HashMap<String, (u128, String)>,
    /// Console commands to run at the next tick (game modes).
    commands: Vec<String>,
    /// Messages to players for the next tick (from hooks without a host).
    tells: Vec<(u128, String)>,
    /// Entities that explode: id -> (kind, dimension).
    explosives: HashMap<u32, (String, String)>,
    /// Players who teleported since the last tick (exit-via-teleport).
    teleported: HashSet<u128>,
}

impl State {
    fn new() -> State {
        State {
            guard: Guard::new(Config::default()),
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
        }
    }

    fn region_file(&self, world: &str) -> Option<PathBuf> {
        Some(self.data_dir.as_ref()?.join("worlds").join(world).join("regions.yml"))
    }

    /// Reads every world's region file once the server directory is known.
    fn load_all(&mut self, host: &dyn Host) {
        if self.loaded {
            return;
        }
        let Some(world_dir) = host.world_dir() else { return };
        let server = world_dir.parent().map(Path::to_path_buf).unwrap_or(world_dir);
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
                    eprintln!("[worldguard] {} is not readable, not loaded: {e}", file.display());
                    format!("The region file of '{name}' is not readable: {e}")
                }
            },
            Err(_) => {
                self.guard.worlds.entry(name.to_owned()).or_insert_with(|| World::new(name));
                format!("No region file for '{name}' yet.")
            }
        }
    }

    fn save_world(&mut self, name: &str) {
        let Some(file) = self.region_file(name) else { return };
        let Some(w) = self.guard.worlds.get_mut(name) else { return };
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

    fn player(&self, p: &NativeServerPlayer) -> Player {
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

#[cfg(not(feature = "worldedit_server"))]
fn new_selections() -> Box<dyn Selections + Send> {
    Box::new(wg::selection::Store::default())
}

#[cfg(feature = "worldedit_server")]
fn new_selections() -> Box<dyn Selections + Send> {
    Box::new(we_selection::WorldEdit)
}

/// A `&`-coded line as Text.
fn line(s: &str) -> Text {
    text::segments(s, 0xFFFFFF)
        .into_iter()
        .fold(Text::empty(), |all, seg| all.append(Text::colored(seg.text, seg.rgb)))
}

fn tell(host: &mut dyn Host, uuid: u128, s: &str) {
    if !s.is_empty() {
        host.tell(uuid, line(s));
    }
}

/// `Overworld` / `Nether` / `End` (the level hooks' dimension, read through `Debug` so this file
/// does not name the engine's type) as an id.
fn dim_id(d: &impl std::fmt::Debug) -> String {
    // UNVERIFIED: the level hooks' `LevelDimension` prints as its variant name.
    let s = format!("{d:?}");
    match s.as_str() {
        "Overworld" => "minecraft:overworld".into(),
        "Nether" => "minecraft:the_nether".into(),
        "End" => "minecraft:the_end".into(),
        other if other.contains(':') => other.trim_matches('"').to_owned(),
        other => format!("minecraft:{}", other.to_ascii_lowercase()),
    }
}

/// Reads a `[f64; 3]` event field whether the event holds it by value or as `&mut`
/// (UNVERIFIED which one the generator makes of a `&mut [f64; 3]` argument).
fn arr3<T: AsRef<[f64]>>(t: &T) -> [f64; 3] {
    let s = t.as_ref();
    [s[0], s[1], s[2]]
}

/// UNVERIFIED: the level hooks' `BlockPos` has public `x`, `y`, `z`.
macro_rules! bp {
    ($p:expr) => {
        BlockPos::new($p.x, $p.y, $p.z)
    };
}

/// Moves a player through `native_teleport_player` (usable inside hooks); logs a refusal.
fn teleport(host: &mut dyn Host, player: u128, dimension: &str, pos: [f64; 3], yaw: f32, pitch: f32) -> bool {
    let done = host.teleport_player(player, dimension, pos, yaw, pitch);
    if !done {
        eprintln!(
            "[worldguard] teleport of {} to {dimension} {pos:?} refused",
            wg::region::uuid_string(player)
        );
    }
    done
}

struct KnownPlayers<'a> {
    names: &'a HashMap<String, (u128, String)>,
    host: &'a dyn Host,
    op_level: u8,
}

impl Players for KnownPlayers<'_> {
    fn has_permission(&self, uuid: u128, node: &str) -> bool {
        self.host.has_permission(uuid, node, self.op_level)
    }
    fn resolve(&self, name: &str) -> Option<(u128, String)> {
        self.names.get(&name.to_ascii_lowercase()).cloned()
    }
    fn name_of(&self, uuid: u128) -> Option<String> {
        self.names.values().find(|(u, _)| *u == uuid).map(|(_, n)| n.clone())
    }
}

impl WorldGuard {
    fn load_config(&self, text: &str, what: &str) {
        match Config::parse(text) {
            Ok(cfg) => {
                lock(&self.st).guard.cfg = cfg;
                eprintln!("[worldguard] config {what}");
            }
            Err(e) => eprintln!("[worldguard] config not {what}, keeping the old one: {e}"),
        }
    }

    /// Runs `/rg ...` for a player (`sender`) or the console; returns the reply lines.
    fn region_command(&self, host: &mut dyn Host, sender: Option<&NativeServerPlayer>, args: &str) -> Vec<String> {
        let mut st = lock(&self.st);
        st.load_all(host);
        let player = sender.map(|p| st.player(p));
        let height = host.world_height(); // UNVERIFIED: (min_y, height) of the current level.
        let height = (height.0, height.0 + height.1 - 1);
        let names = st.names.clone();
        let op_level = st.guard.cfg.op_level;
        let known = KnownPlayers { names: &names, host: &*host, op_level };
        let State { guard, selections, .. } = &mut *st;
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
                Effect::Teleport { player, region, dimension, pos, yaw, pitch } => {
                    teleports.push((player, region, dimension, pos, yaw, pitch))
                }
                Effect::Save(w) => st.save_world(&w),
                Effect::Load(w) => lines.push(format!("&7{}", st.load_world(&w))),
            }
        }
        if st.guard.worlds.values().any(|w| w.dirty) && st.save_at.is_none() {
            st.save_at = Some(st.ticks + st.guard.cfg.save_delay_seconds as u64 * TPS);
        }
        // Not under the lock: a teleport may be heard by this mod's own teleport hooks.
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
    fn denied(host: &mut dyn Host, p: &Player, msg: &str) -> Flow {
        static LAST: std::sync::OnceLock<Mutex<HashMap<u128, std::time::Instant>>> = std::sync::OnceLock::new();
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
            let players = host.server_players();
            let health = players.iter().find(|x| x.uuid == p.subject.uuid).map_or(20.0, |x| x.health);
            let msg = msg
                .replace("%online%", &players.len().to_string())
                .replace("%health%", &format!("{:?}", health as f64));
            tell(host, p.subject.uuid, &msg);
        }
        Flow::Cancel
    }
}

impl Hooks for WorldGuard {
    fn register_configs(&self, _host: &mut dyn Host, event: &mut RegisterConfigsEvent<'_>) {
        event
            .configs
            .push(NativeConfig::new(MOD_ID, NativeConfigKind::Common, DEFAULT_TOML));
    }

    fn config_loading(&self, _host: &mut dyn Host, event: &mut ConfigLoadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            self.load_config(&event.config.text, "loaded");
        }
    }

    fn config_reloading(&self, _host: &mut dyn Host, event: &mut ConfigReloadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            self.load_config(&event.config.text, "reloaded");
        }
    }

    fn server_stopping(&self, _host: &mut dyn Host, _event: &mut ServerStoppingEvent) {
        lock(&self.st).save_dirty();
    }

    fn register_commands(&self, _host: &mut dyn Host, event: &mut RegisterCommandsEvent<'_>) {
        // One usage per subcommand path, so the client completes names and arguments. The
        // dispatch in `perform_command` takes the typed line whichever usage it matched, so no
        // catch-all is needed. Level 0 for `rg`: non-operators may run the subcommands that
        // `player-commands` lists (`run` checks the rest).
        for root in ["rg", "region", "regions"] {
            event.commands.push(NativeCommand::new(root));
            // Full tree on /rg only; the long aliases take free text (smaller command packet).
            if root == "rg" {
                for usage in commands::usages() {
                    event.commands.push(NativeCommand::new(&format!("{root} {usage}")));
                }
            } else {
                event.commands.push(NativeCommand::new(&format!("{root} <args:text>")));
            }
        }
        for root in ["wg", "worldguard"] {
            event.commands.push(NativeCommand::op(root, 2));
            for usage in WG_USAGES {
                event.commands.push(NativeCommand::op(&format!("{root} {usage}"), 2));
            }
        }
    }

    fn perform_command(&self, host: &mut dyn Host, event: &mut PerformCommandEvent<'_>) -> Flow {
        let (root, args) = event.command.split_once(' ').unwrap_or((event.command, ""));
        let sender = event.player.clone();
        match root {
            "rg" | "region" | "regions" => {
                for l in self.region_command(host, Some(&sender), args) {
                    tell(host, sender.uuid, &l);
                }
                Flow::Cancel
            }
            "wg" | "worldguard" => {
                for l in self.wg_command(host, args) {
                    tell(host, sender.uuid, &l);
                }
                Flow::Cancel
            }
            _ => Flow::Continue,
        }
    }

    fn perform_console_command(&self, host: &mut dyn Host, event: &mut PerformConsoleCommandEvent<'_>) -> Flow {
        let cmd = event.command.trim_start_matches('/');
        let (root, args) = cmd.split_once(' ').unwrap_or((cmd, ""));
        let lines = match root {
            "rg" | "region" | "regions" => self.region_command(host, None, args),
            "wg" | "worldguard" => self.wg_command(host, args),
            _ => return Flow::Continue,
        };
        for l in lines {
            eprintln!("[worldguard] {}", text::plain(&l));
        }
        Flow::Cancel
    }

    fn tick(&self, host: &mut dyn Host, _event: &mut TickEvent) {
        let mut st = lock(&self.st);
        st.load_all(host);
        st.ticks += 1;
        let players = host.server_players();
        st.by_entity = players.iter().map(|p| (p.entity_id, p.clone())).collect();
        st.by_uuid = players.iter().map(|p| (p.uuid, p.clone())).collect();
        for p in &players {
            st.names.insert(p.name.to_ascii_lowercase(), (p.uuid, p.name.clone()));
        }
        let teleported = std::mem::take(&mut st.teleported);
        let online = players.len();
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
                        online,
                        id,
                        health: np.health,
                    },
                )
            };
            let State { guard, tracker, .. } = &mut *st;
            let events = tracker.update(guard, &p, np.yaw, np.pitch, teleported.contains(&np.uuid), &fill);
            for e in events {
                match e {
                    MoveEvent::Tell(s) => tells.push((np.uuid, s)),
                    MoveEvent::Title(a, b) => titles.push((np.uuid, a, b)),
                    MoveEvent::PushBack { to, yaw, pitch } => {
                        pushes.push((np.uuid, np.dimension.clone(), to, yaw, pitch))
                    }
                    MoveEvent::Notify(s) => ops_notes.push(s),
                    MoveEvent::GameMode(m) => {
                        let mode = ["survival", "creative", "adventure", "spectator"][m.clamp(0, 3) as usize];
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
            if !host.run_command(&c) {
                eprintln!("[worldguard] command failed: {c}");
            }
        }
        for (u, s) in tells {
            tell(host, u, &s);
        }
        for (u, a, b) in titles {
            // Vanilla's default times (fade in, stay, fade out), as WorldGuard shows them.
            host.title(u, &line(&a), &line(&b), 10, 70, 20);
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

    fn player_logged_in(&self, host: &mut dyn Host, event: &mut PlayerLoggedInEvent<'_>) {
        let mut st = lock(&self.st);
        st.names
            .insert(event.name.to_ascii_lowercase(), (event.player, event.name.to_owned()));
        if st.guard.cfg.announce_bypass_status
            && let Some(np) = host.server_players().into_iter().find(|p| p.uuid == event.player)
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

    fn player_logged_out(&self, _host: &mut dyn Host, event: &mut PlayerLoggedOutEvent<'_>) {
        let mut st = lock(&self.st);
        st.tracker.forget(event.player);
    }

    // -- blocks ----------------------------------------------------------------------------

    fn destroy_block(&self, host: &mut dyn Host, event: &mut DestroyBlockEvent) -> Flow {
        let Some(np) = host.acting_player() else { return Flow::Continue };
        let st = lock(&self.st);
        let p = st.player(&np);
        match st.guard.break_block(&p, event.pos.into()) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
            }
        }
    }

    fn right_click_block(&self, host: &mut dyn Host, event: &mut RightClickBlockEvent) -> Flow {
        let Some(np) = host.acting_player() else { return Flow::Continue };
        let block = host
            .level(&np.dimension)
            .and_then(|l| host.level_block(l, event.pos))
            .unwrap_or_default();
        let st = lock(&self.st);
        let p = st.player(&np);
        // The region wand: operators see what is here.
        let wand = host.held().is_some_and(|(item, _, _)| item == st.guard.cfg.wand);
        if wand && st.guard.is_admin(&p) {
            let ids = st
                .guard
                .world(&p.dimension)
                .map(|w| wg::query::Query::new(w).ids_at(event.pos.into()))
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
        match st.guard.use_block(&p, event.pos.into(), &block) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
            }
        }
    }

    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        let Some(np) = host.acting_player() else { return Flow::Continue };
        let item = event.stack.item.to_string();
        let block = host
            .level(&np.dimension)
            .and_then(|l| host.level_block(l, event.pos))
            .unwrap_or_default();
        let st = lock(&self.st);
        let p = st.player(&np);
        // A block item is checked by `block_place`, where it would place.
        match st.guard.use_item_on(&p, event.pos.into(), event.face, &item, &block) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
            }
        }
    }

    /// Before a block item places (hooks-all `native_block_place`): a cancel places nothing and
    /// uses nothing up, so there is nothing to undo.
    fn block_place(&self, host: &mut dyn Host, event: &mut BlockPlaceEvent<'_>) -> Flow {
        let pos = event.pos;
        self.player_check(host, event.player, |g, p| g.place_block(p, pos.into()))
    }

    fn bucket_empty(&self, host: &mut dyn Host, event: &mut BucketEmptyEvent<'_>) -> Flow {
        self.player_check(host, event.player, |g, p| g.bucket(p, event.pos.into()))
    }

    fn fill_bucket(&self, host: &mut dyn Host, event: &mut FillBucketEvent<'_>) -> Flow {
        let Some(np) = host.acting_player() else { return Flow::Continue };
        self.player_check(host, np.uuid, |g, p| g.bucket_fill(p, event.pos.into()))
    }

    fn hanging_place(&self, host: &mut dyn Host, event: &mut HangingPlaceEvent<'_>) -> Flow {
        self.player_check(host, event.player, |g, p| g.hanging_place(p, event.pos.into()))
    }

    fn hanging_break(&self, host: &mut dyn Host, event: &mut HangingBreakEvent<'_>) -> Flow {
        let st = lock(&self.st);
        match event.player {
            Some(u) => {
                drop(st);
                self.player_check(host, u, |g, p| g.hanging_break(Some(p), &p.dimension.clone(), event.pos.into(), event.kind))
            }
            None => {
                // No dimension in the event (GAPS.md): the overworld's regions decide.
                let dim = "minecraft:overworld";
                if st.guard.hanging_break(None, dim, event.pos.into(), event.kind).is_ok() {
                    Flow::Continue
                } else {
                    Flow::Cancel
                }
            }
        }
    }

    fn tnt_primed(&self, host: &mut dyn Host, event: &mut TntPrimedEvent) -> Flow {
        match event.player {
            Some(u) => self.player_check(host, u, |g, p| g.tnt_prime(Some(p), &p.dimension.clone(), event.pos.into())),
            None => {
                let st = lock(&self.st);
                // No dimension in the event (GAPS.md).
                if st.guard.tnt_prime(None, "minecraft:overworld", event.pos.into()).is_ok() {
                    Flow::Continue
                } else {
                    Flow::Cancel
                }
            }
        }
    }

    fn explosion_detonate(&self, host: &mut dyn Host, event: &mut ExplosionDetonateEvent<'_>) {
        let st = lock(&self.st);
        // The blast's source is not in the event (GAPS.md): take the nearest tracked explosive.
        let mut best: Option<(f64, String)> = None;
        for (id, (kind, dim)) in &st.explosives {
            if dim != event.dimension {
                continue;
            }
            let Some(pos) = host.entity(*id).and_then(|e| host.entity_position(e)) else { continue };
            let d = (0..3).map(|i| (pos[i] - event.center[i]).powi(2)).sum::<f64>();
            if d <= 9.0 && best.as_ref().is_none_or(|(b, _)| d < *b) {
                best = Some((d, kind.clone()));
            }
        }
        let blast = Blast::from_kind(best.as_ref().map(|(_, k)| k.as_str()));
        st.guard.explosion(event.dimension, event.center, blast, event.blocks);
    }

    fn piston_move(&self, _level: &dyn LevelHost, event: &mut PistonMoveEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let dim = dim_id(&event.dimension);
        let push: Vec<BlockPos> = event.push.iter().map(|p| bp!(p)).collect();
        let destroy: Vec<BlockPos> = event.destroy.iter().map(|p| bp!(p)).collect();
        if st.guard.piston(&dim, bp!(event.piston), event.direction, &push, &destroy) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    fn block_spread(&self, _level: &dyn LevelHost, event: &mut BlockSpreadEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let dim = dim_id(&event.dimension);
        let ok = if event.kind == "fire" {
            st.guard.fire_spread(&dim, bp!(event.to))
        } else {
            // Which fluid is not told and the level host has no block names (GAPS.md):
            // water-flow and lava-flow both apply to any fluid.
            st.guard.fluid_flow(&dim, "water", bp!(event.from), bp!(event.to))
                && st.guard.fluid_flow(&dim, "lava", bp!(event.from), bp!(event.to))
        };
        if ok { Flow::Continue } else { Flow::Cancel }
    }

    fn block_ignite(&self, _level: &dyn LevelHost, event: &mut BlockIgniteEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let dim = dim_id(&event.dimension);
        let p = event.player.and_then(|u| st.cached(u));
        match st.guard.ignite(p.as_ref(), &dim, bp!(event.pos), event.cause) {
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
        if st.guard.burn(&dim_id(&event.dimension), bp!(event.pos)) { Flow::Continue } else { Flow::Cancel }
    }

    fn leaves_decay(&self, _level: &dyn LevelHost, event: &mut LeavesDecayEvent) -> Flow {
        let st = lock(&self.st);
        if st.guard.leaf_decay(&dim_id(&event.dimension), bp!(event.pos)) { Flow::Continue } else { Flow::Cancel }
    }

    // -- entities --------------------------------------------------------------------------

    fn attack_entity(&self, host: &mut dyn Host, event: &mut AttackEntityEvent<'_>) -> Flow {
        let Some(np) = host.acting_player() else { return Flow::Continue };
        let st = lock(&self.st);
        let p = st.player(&np);
        let victim = if event.kind == "minecraft:player" {
            st.by_entity.get(&event.entity).map(|v| st.player(v))
        } else {
            None
        };
        match st.guard.attack_entity(&p, event.kind, event.pos, victim.as_ref()) {
            Ok(()) => Flow::Continue,
            Err(d) => {
                drop(st);
                Self::denied(host, &p, &d.message)
            }
        }
    }

    fn interact_entity(&self, host: &mut dyn Host, event: &mut InteractEntityEvent<'_>) -> Flow {
        let Some(np) = host.acting_player() else { return Flow::Continue };
        let at = host
            .entity(event.entity as u32)
            .and_then(|e| host.entity_position(e))
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

    fn armor_stand_manipulate(&self, host: &mut dyn Host, event: &mut ArmorStandManipulateEvent<'_>) -> Flow {
        let at = host.entity(event.stand_id as u32).and_then(|e| host.entity_position(e));
        self.player_check(host, event.player, |g, p| {
            g.interact_entity(p, "minecraft:armor_stand", at.unwrap_or(p.pos))
        })
    }

    fn entity_mount(&self, host: &mut dyn Host, event: &mut EntityMountEvent<'_>) -> Flow {
        if !event.mounting || event.kind != "minecraft:player" {
            return Flow::Continue;
        }
        let at = host.entity(event.vehicle).and_then(|e| host.entity_position(e));
        let uuid = {
            let st = lock(&self.st);
            st.by_entity.get(&(event.entity as i32)).map(|p| p.uuid)
        };
        let Some(uuid) = uuid else { return Flow::Continue };
        let vk = event.vehicle_kind.to_owned();
        self.player_check(host, uuid, |g, p| g.mount(p, &vk, at.unwrap_or(p.pos)))
    }

    fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
        if event.kind != "minecraft:player" {
            return Flow::Continue;
        }
        let st = lock(&self.st);
        let Some(victim) = st.by_entity.get(&event.entity).map(|v| st.player(v)) else {
            return Flow::Continue;
        };
        let attacker = st.by_entity.get(&event.causing).map(|a| st.player(a));
        let by_mob = event.causing != 0 && attacker.is_none() && event.causing != event.entity;
        if st.guard.damage_player(&victim, event.damage_type, attacker.as_ref(), by_mob) {
            Flow::Continue
        } else {
            Flow::Cancel
        }
    }

    fn entity_join_level(&self, _host: &mut dyn Host, event: &mut EntityJoinLevelEvent<'_>) -> Flow {
        let mut st = lock(&self.st);
        let k = event.kind.strip_prefix("minecraft:").unwrap_or(event.kind);
        if matches!(
            k,
            "tnt" | "tnt_minecart" | "creeper" | "fireball" | "small_fireball" | "dragon_fireball" | "wither_skull"
                | "wither" | "ender_dragon" | "end_crystal" | "breeze_wind_charge" | "wind_charge"
        ) {
            st.explosives
                .insert(event.entity, (event.kind.to_owned(), event.dimension.to_owned()));
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

    fn entity_leave_level(&self, _host: &mut dyn Host, event: &mut EntityLeaveLevelEvent<'_>) {
        // Kept one tick longer would be nicer for the blast lookup; the explosion hook runs
        // before the exploding entity is removed (UNVERIFIED).
        lock(&self.st).explosives.remove(&event.entity);
    }

    // -- players ---------------------------------------------------------------------------

    fn item_picked_up(&self, host: &mut dyn Host, event: &mut ItemPickedUpEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else { return Flow::Continue };
        let _ = host;
        if st.guard.item_pickup(&p).is_ok() { Flow::Continue } else { Flow::Cancel }
    }

    /// `item-drop` (hooks-all `native_item_dropped_v2`): a cancel puts the stack back.
    fn player_portal(&self, host: &mut dyn Host, event: &mut PlayerPortalEvent<'_>) -> Flow {
        if event.cause != "nether_portal" {
            return Flow::Continue;
        }
        let denied = {
            let st = lock(&self.st);
            if !st.guard.cfg.nether_portal_protection {
                return Flow::Continue;
            }
            let Some(p) = st.cached(event.player) else { return Flow::Continue };
            // Overworld to Nether divides by 8, the way back multiplies: the portal is built there.
            let scale = if event.to_dimension.ends_with("the_nether") { 0.125 } else { 8.0 };
            let at = [p.pos[0] * scale, p.pos[1], p.pos[2] * scale];
            let dest = Player { dimension: event.to_dimension.to_owned(), pos: at, ..p };
            st.guard.place_block(&dest, BlockPos::of(at)).err()
        };
        match denied {
            Some(d) => {
                tell(host, event.player, &d.message);
                Flow::Cancel
            }
            None => Flow::Continue,
        }
    }

    fn item_dropped_v2(&self, host: &mut dyn Host, event: &mut ItemDroppedV2Event<'_>) -> Flow {
        self.player_check(host, event.player, |g, p| g.item_drop(p))
    }

    fn ender_pearl_teleport(&self, host: &mut dyn Host, event: &mut EnderPearlTeleportEvent<'_>) -> Flow {
        let target = arr3(&event.target);
        let f = self.player_check(host, event.player, |g, p| g.ender_pearl(p, target));
        if f == Flow::Continue {
            lock(&self.st).teleported.insert(event.player);
        }
        f
    }

    fn chorus_fruit_teleport(&self, host: &mut dyn Host, event: &mut ChorusFruitTeleportEvent<'_>) -> Flow {
        let target = arr3(&event.target);
        let f = self.player_check(host, event.player, |g, p| g.chorus(p, target));
        if f == Flow::Continue {
            lock(&self.st).teleported.insert(event.player);
        }
        f
    }

    /// `entry`/`exit` before a move is applied (hooks-all `native_player_move`): a cancel keeps the
    /// player where they were. Teleports and moves this hook does not hear (riders) are still
    /// caught by the tick's check, which pushes them back.
    fn player_move(&self, event: &mut PlayerMoveEvent<'_>) -> Flow {
        let mut st = lock(&self.st);
        let Some(np) = st.by_uuid.get(&event.player).cloned() else { return Flow::Continue };
        let mut p = st.player(&np);
        p.pos = *event.to;
        let world = st.guard.cfg.world_of(&p.dimension);
        let online = st.by_uuid.len();
        let fill = |s: &str, id: &str| {
            text::fill(s, &text::Placeholders { name: &np.name, world: &world, online, id, health: np.health })
        };
        let Some(msg) = wg::movement::check_step(&st.guard, &p, event.from, &fill) else {
            return Flow::Continue;
        };
        // No host here: told at the next tick, once however many moves were refused.
        if !st.tells.iter().any(|(u, m)| *u == np.uuid && *m == msg) {
            st.tells.push((np.uuid, msg));
        }
        Flow::Cancel
    }

    fn player_teleported(&self, _host: &mut dyn Host, event: &mut PlayerTeleportedEvent<'_>) {
        lock(&self.st).teleported.insert(event.player);
    }

    fn can_player_sleep(&self, _host: &mut dyn Host, event: &mut CanPlayerSleepEvent<'_>) {
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else { return };
        if let Err(d) = st.guard.sleep(&p, event.pos.into()) {
            *event.problem = Some(text::plain(&d.message));
        }
    }

    fn player_chat(&self, event: &mut PlayerChatEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else { return Flow::Continue };
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

    fn player_command(&self, _host: &mut dyn Host, event: &mut PlayerCommandEvent<'_>) -> Flow {
        let st = lock(&self.st);
        let Some(p) = st.cached(event.player) else { return Flow::Continue };
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

impl WorldGuard {
    /// Runs a protection check for the player `uuid` and tells them when it is denied.
    fn player_check(
        &self,
        host: &mut dyn Host,
        uuid: u128,
        check: impl FnOnce(&Guard, &Player) -> wg::guard::Outcome,
    ) -> Flow {
        let st = lock(&self.st);
        let p = match host.acting_player().filter(|a| a.uuid == uuid) {
            Some(np) => st.player(&np),
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
    fn wg_command(&self, host: &mut dyn Host, args: &str) -> Vec<String> {
        let mut st = lock(&self.st);
        st.load_all(host);
        let args = args.trim();
        let (sub, arg) = args.split_once(' ').map_or((args, ""), |(s, a)| (s, a.trim()));
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
                format!("&dWorldGuard {}", env!("CARGO_PKG_VERSION")),
                "&dhttp://www.enginehub.org".to_owned(),
                "&7----------- Platforms -----------".to_owned(),
                "&7* Horizon".to_owned(),
            ],
            "report" | "profile" | "stopprofile" | "debug" => {
                vec![format!("&c/wg {sub} is not available yet.")]
            }
            _ => vec!["&cUnknown subcommand. Use /wg reload, flushstates, running or version.".into()],
        }
    }
}
