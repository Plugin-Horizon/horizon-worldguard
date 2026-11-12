//! Movement flags, checked once a tick from each player's position: `entry`/`exit` (with
//! `exit-override`, `exit-via-teleport` and the deny messages), `greeting`/`farewell` and their
//! titles, `notify-enter`/`notify-leave`, and `game-mode`. The rules are those of WorldGuard's
//! session handlers (`EntryFlag`, `ExitFlag`, `GreetingFlag`, `FarewellFlag`, `NotifyEntryFlag`,
//! `NotifyExitFlag`, `GameModeFlag`).

use super::flags::FlagValue;
use super::geom::BlockPos;
use super::guard::{Guard, Player};
use super::query::{Actor, Query};
use super::region::uuid_string;
use super::text;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum MoveEvent {
    /// A chat line to the player (colour codes included).
    Tell(String),
    /// A title (and subtitle) to the player.
    Title(String, String),
    /// Put the player back where they were allowed to be.
    PushBack { to: [f64; 3], yaw: f32, pitch: f32 },
    /// Tell operators (`notify-enter`/`notify-leave`).
    Notify(String),
    /// Set the player's game mode (0..3).
    GameMode(i32),
}

#[derive(Clone, Debug)]
struct Last {
    dimension: String,
    pos: [f64; 3],
    block: BlockPos,
    regions: Vec<String>,
    /// The last spot they were allowed at.
    safe: ([f64; 3], f32, f32),
    /// Game mode before a `game-mode` flag changed it.
    mode_before: Option<i32>,
    /// The messages the greeting and farewell handlers have sent for (their "stacks").
    greeting: Vec<String>,
    greeting_title: Vec<String>,
    farewell: Vec<String>,
    farewell_title: Vec<String>,
    /// `notify-enter` / `notify-leave` as last seen.
    notify_enter: Option<bool>,
    notify_leave: Option<bool>,
}

#[derive(Default)]
pub struct Tracker {
    last: HashMap<u128, Last>,
}

/// Deny messages are shown at most every two seconds per player and kind (entry, exit).
const MESSAGE_THRESHOLD: Duration = Duration::from_secs(2);

fn throttled(player: u128, exit: bool) -> bool {
    static LAST: std::sync::OnceLock<Mutex<HashMap<(u128, bool), Instant>>> = std::sync::OnceLock::new();
    let mut m = LAST
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    match m.get(&(player, exit)) {
        Some(t) if now.duration_since(*t) < MESSAGE_THRESHOLD => true,
        _ => {
            m.insert((player, exit), now);
            false
        }
    }
}

fn strings(q: &Query<'_>, at: BlockPos, flag: &str, a: &Actor<'_>) -> Option<String> {
    q.value(at, flag, a).and_then(|v| v.as_str().map(str::to_owned))
}

/// `queryAllValues` of a string flag, in order, without repeats.
fn all_strings(q: &Query<'_>, at: BlockPos, flag: &str, a: &Actor<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in q.values(at, flag, a) {
        if let Some(s) = v.as_str()
            && !out.iter().any(|o| o == s)
        {
            out.push(s.to_owned());
        }
    }
    out
}

/// The `flag` string of every region in the set at `at`, the messages the handlers add to their
/// stacks.
fn region_strings(q: &Query<'_>, at: BlockPos, flag: &str) -> Vec<String> {
    q.regions_at(at)
        .iter()
        .filter_map(|r| r.flag(flag).and_then(|v| v.as_str().map(str::to_owned)))
        .collect()
}

/// `MessagingUtil.sendStringToTitle`: the first line is the title, the rest the subtitle.
fn split_title(s: &str) -> (String, String) {
    let t = s.replace("\\n", "\n");
    match t.split_once('\n') {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (t, String::new()),
    }
}

fn game_mode_of(s: &str) -> Option<i32> {
    match s.strip_prefix("minecraft:").unwrap_or(s).to_ascii_lowercase().as_str() {
        "survival" => Some(0),
        "creative" => Some(1),
        "adventure" => Some(2),
        "spectator" => Some(3),
        _ => None,
    }
}

impl Tracker {
    pub fn forget(&mut self, player: u128) {
        self.last.remove(&player);
    }

    /// Forgets every player's last position (`/wg flushstates` without a player).
    pub fn forget_all(&mut self) {
        self.last.clear();
    }

    /// Checks `p`'s move since the last call. `teleported`: the move was a teleport (pearl,
    /// command), for `exit-via-teleport`. `fill` replaces the placeholders of a message (its
    /// second argument is the player's id, for `%id%`).
    pub fn update(
        &mut self,
        guard: &Guard,
        p: &Player,
        yaw: f32,
        pitch: f32,
        teleported: bool,
        fill: &dyn Fn(&str, &str) -> String,
    ) -> Vec<MoveEvent> {
        let mut out = Vec::new();
        let block = p.block();
        let world = guard.world(&p.dimension);
        let ids: Vec<String> = world
            .map(|w| Query::new(w).regions_at(block).iter().map(|r| r.id.clone()).collect())
            .unwrap_or_default();
        let uuid = uuid_string(p.subject.uuid);
        let me = Actor::Player(&p.subject);
        let Some(prev) = self.last.get(&p.subject.uuid).cloned() else {
            // WorldGuard's `initialize`: the farewell stacks start with what is here.
            let (farewell, farewell_title) = match world {
                Some(w) => {
                    let q = Query::new(w);
                    (all_strings(&q, block, "farewell", &me), all_strings(&q, block, "farewell-title", &me))
                }
                None => (Vec::new(), Vec::new()),
            };
            self.last.insert(
                p.subject.uuid,
                Last {
                    dimension: p.dimension.clone(),
                    pos: p.pos,
                    block,
                    regions: ids,
                    safe: (p.pos, yaw, pitch),
                    mode_before: None,
                    greeting: Vec::new(),
                    greeting_title: Vec::new(),
                    farewell,
                    farewell_title,
                    notify_enter: world.and_then(|w| bool_flag(&Query::new(w), block, "notify-enter", &me)),
                    notify_leave: world.and_then(|w| bool_flag(&Query::new(w), block, "notify-leave", &me)),
                },
            );
            return out;
        };
        if prev.block == block && prev.dimension == p.dimension {
            if let Some(l) = self.last.get_mut(&p.subject.uuid) {
                l.pos = p.pos;
            }
            return out;
        }
        let same_dim = prev.dimension == p.dimension;
        if !guard.cfg.enable || !guard.cfg.use_player_move_event || world.is_none() {
            let mut keep = prev.clone();
            keep.dimension = p.dimension.clone();
            keep.pos = p.pos;
            keep.block = block;
            keep.regions = ids;
            keep.safe = (p.pos, yaw, pitch);
            self.last.insert(p.subject.uuid, keep);
            return out;
        }
        let w = world.unwrap();
        let q = Query::new(w);
        let entered: Vec<&String> = ids.iter().filter(|i| !prev.regions.contains(i)).collect();
        let left: Vec<&String> = if same_dim {
            prev.regions.iter().filter(|i| !ids.contains(i)).collect()
        } else {
            Vec::new()
        };
        let bypass = guard.bypasses(p);
        let changed = !entered.is_empty() || !left.is_empty() || !same_dim;

        if !bypass
            && same_dim
            && let Some(msg) = refused_step(guard, &q, p, prev.block, changed, teleported)
        {
            if let Some(m) = msg {
                out.push(MoveEvent::Tell(m));
            }
            out.push(push_back(&prev));
            return out;
        }

        let mut next = prev.clone();
        // Greeting: the first message that is not on the stack; the stack becomes what applies
        // here plus what every region of the set says.
        for (flag, title) in [("greeting", false), ("greeting-title", true)] {
            let messages = all_strings(&q, block, flag, &me);
            let stack = if title { &prev.greeting_title } else { &prev.greeting };
            if let Some(m) = messages.iter().find(|m| !stack.contains(m)) {
                let filled = fill(m, &uuid);
                if title {
                    let (a, b) = split_title(&filled);
                    out.push(MoveEvent::Title(a, b));
                } else {
                    for line in text::lines_of(&filled) {
                        out.push(MoveEvent::Tell(line));
                    }
                }
            }
            let mut new_stack = messages.clone();
            if !new_stack.is_empty() {
                for m in region_strings(&q, block, flag) {
                    if !new_stack.contains(&m) {
                        new_stack.push(m);
                    }
                }
            }
            if title {
                next.greeting_title = new_stack;
            } else {
                next.greeting = new_stack;
            }
        }
        // Farewell: the first message of the old stack that no longer applies.
        for (flag, title) in [("farewell", false), ("farewell-title", true)] {
            let mut messages = all_strings(&q, block, flag, &me);
            if !messages.is_empty() {
                for m in region_strings(&q, block, flag) {
                    if !messages.contains(&m) {
                        messages.push(m);
                    }
                }
            }
            let stack = if title { &prev.farewell_title } else { &prev.farewell };
            if let Some(m) = stack.iter().find(|m| !messages.contains(m)) {
                let filled = fill(m, &uuid);
                if title {
                    let (a, b) = split_title(&filled);
                    out.push(MoveEvent::Title(a, b));
                } else {
                    for line in text::lines_of(&filled) {
                        out.push(MoveEvent::Tell(line));
                    }
                }
            }
            if title {
                next.farewell_title = messages;
            } else {
                next.farewell = messages;
            }
        }
        // notify-enter / notify-leave act when the regions changed.
        if changed {
            let now_enter = bool_flag(&q, block, "notify-enter", &me);
            if now_enter.is_some() && now_enter != prev.notify_enter && now_enter == Some(true) {
                let list: Vec<String> = q.regions_at(block).iter().map(|r| r.id.clone()).collect();
                out.push(MoveEvent::Notify(format!(
                    "&7WG: &d{}&6 entered NOTIFY region: {}",
                    p.subject.name,
                    list.join(", ")
                )));
            }
            next.notify_enter = now_enter;
            let now_leave = bool_flag(&q, block, "notify-leave", &me);
            if now_leave.is_none() && prev.notify_leave == Some(true) {
                out.push(MoveEvent::Notify(format!("&7WG: &d{}&6 left NOTIFY region", p.subject.name)));
            }
            next.notify_leave = now_leave;
        }

        // game-mode: set on entry, restore when no region sets one any more.
        let mut mode_before = prev.mode_before;
        let mode_new = strings(&q, block, "game-mode", &me).as_deref().and_then(game_mode_of);
        match mode_new {
            Some(m) if m != p.game_mode && !bypass => {
                if mode_before.is_none() {
                    mode_before = Some(p.game_mode);
                }
                out.push(MoveEvent::GameMode(m));
            }
            None => {
                if let Some(m) = mode_before.take()
                    && m != p.game_mode
                {
                    out.push(MoveEvent::GameMode(m));
                }
            }
            _ => {}
        }
        next.dimension = p.dimension.clone();
        next.pos = p.pos;
        next.block = block;
        next.regions = ids;
        next.safe = (p.pos, yaw, pitch);
        next.mode_before = mode_before;
        self.last.insert(p.subject.uuid, next);
        out
    }
}

fn bool_flag(q: &Query<'_>, at: BlockPos, flag: &str, a: &Actor<'_>) -> Option<bool> {
    match q.value(at, flag, a) {
        Some(FlagValue::Bool(b)) => Some(b),
        _ => None,
    }
}

/// `entry`/`exit` for one move, as the `player_move` hook hears it before the move is applied:
/// from `from` to `p.pos`, both in `p.dimension`. The filled deny message when the move must be
/// refused (the hook cancels it and the player stays at `from`); `None` lets it through.
pub fn check_step(guard: &Guard, p: &Player, from: [f64; 3], _fill: &dyn Fn(&str, &str) -> String) -> Option<String> {
    if !guard.cfg.enable || !guard.cfg.use_player_move_event || guard.bypasses(p) {
        return None;
    }
    let from_block = BlockPos::of(from);
    if from_block == p.block() {
        return None;
    }
    let q = Query::new(guard.world(&p.dimension)?);
    let from_ids: Vec<String> = q.regions_at(from_block).iter().map(|r| r.id.clone()).collect();
    let ids: Vec<String> = q.regions_at(p.block()).iter().map(|r| r.id.clone()).collect();
    let changed = from_ids != ids;
    // Unset: the step is allowed. Some(None): refused without a message (throttled).
    refused_step(guard, &q, p, from_block, changed, false).map(|m| m.unwrap_or_default())
}

/// The `entry`/`exit` rules of a step from the block `from` to `p`'s block in the same
/// dimension. `Some(message)` when refused (the message is `None` while the two second delay
/// since the last one has not passed); `None` lets the step through.
fn refused_step(
    guard: &Guard,
    q: &Query<'_>,
    p: &Player,
    from: BlockPos,
    changed: bool,
    teleported: bool,
) -> Option<Option<String>> {
    let me = Actor::Player(&p.subject);
    let to = p.block();
    let with_message = |flag: &str, at: BlockPos, default: &str, exit: bool| -> Option<String> {
        if throttled(p.subject.uuid, exit) {
            return None;
        }
        let m = strings(q, at, flag, &me).unwrap_or_else(|| default.to_owned());
        if m.is_empty() { None } else { Some(text::color_macros(&m)) }
    };
    // EntryFlag: wherever the player ends up, entry must be allowed there.
    if !q.test_state(to, &["entry"], &me) {
        return Some(with_message("entry-deny-message", to, &guard.cfg.entry_deny_message, false));
    }
    // ExitFlag: the exit state changes from deny to allow while leaving.
    if changed {
        let last_allowed = q.test_state(from, &["exit"], &me);
        let allowed = q.test_state(to, &["exit"], &me);
        let last_denied_state = q.query_state(from, &["exit"], &me) != q.query_state(to, &["exit"], &me);
        if last_denied_state && allowed && !last_allowed {
            let via_teleport = teleported && q.test_state(from, &["exit-via-teleport"], &me);
            if !via_teleport {
                let override_exit = matches!(q.value(to, "exit-override", &me), Some(FlagValue::Bool(true)));
                if !override_exit {
                    return Some(with_message("exit-deny-message", from, &guard.cfg.exit_deny_message, true));
                }
            }
        }
    }
    None
}

fn push_back(prev: &Last) -> MoveEvent {
    let (to, yaw, pitch) = prev.safe;
    MoveEvent::PushBack { to, yaw, pitch }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::Config;
    use super::super::geom::Shape;
    use super::super::region::{Region, Subject};

    fn pu(uuid: u128, x: f64) -> Player {
        Player {
            subject: Subject { uuid, name: "Walker".into(), groups: vec![] },
            op_level: 0,
            dimension: "minecraft:overworld".into(),
            pos: [x, 64.0, 5.0],
            yaw: 0.0,
            pitch: 0.0,
            game_mode: 0,
        }
    }

    fn p(x: f64) -> Player {
        pu(9, x)
    }

    fn guard() -> Guard {
        let mut g = Guard::new(Config::default());
        let w = g.world_mut("minecraft:overworld");
        let mut r = Region::new("town", Shape::cuboid(BlockPos::new(0, 0, 0), BlockPos::new(10, 255, 10)));
        r.set_flag("greeting", Some(FlagValue::Str("&aWelcome to %id%, %name%".into())));
        r.set_flag("farewell", Some(FlagValue::Str("Bye".into())));
        w.put(r);
        let mut v = Region::new("vault", Shape::cuboid(BlockPos::new(20, 0, 0), BlockPos::new(30, 255, 10)));
        v.set_flag("entry", Some(FlagValue::State(false)));
        w.put(v);
        g
    }

    fn fill(s: &str, id: &str) -> String {
        s.replace("%id%", id).replace("%name%", "Walker")
    }

    #[test]
    fn greeting_farewell_entry() {
        let g = guard();
        let mut t = Tracker::default();
        assert!(t.update(&g, &p(-5.0), 0.0, 0.0, false, &fill).is_empty());
        let ev = t.update(&g, &p(1.0), 0.0, 0.0, false, &fill);
        assert_eq!(ev, vec![MoveEvent::Tell("&aWelcome to 00000000-0000-0000-0000-000000000009, Walker".into())]);
        assert!(t.update(&g, &p(2.0), 0.0, 0.0, false, &fill).is_empty());
        let ev = t.update(&g, &p(15.0), 0.0, 0.0, false, &fill);
        assert_eq!(ev, vec![MoveEvent::Tell("Bye".into())]);
        let ev = t.update(&g, &p(20.5), 0.0, 0.0, false, &fill);
        assert_eq!(ev.len(), 2);
        assert!(matches!(ev[1], MoveEvent::PushBack { to, .. } if to[0] == 15.0));
    }

    #[test]
    fn a_step_into_a_no_entry_region_is_refused_before_it_happens() {
        let g = guard();
        // Into the vault (entry deny): refused with the default message.
        let msg = check_step(&g, &pu(21, 20.5), [19.5, 64.0, 5.0], &fill);
        assert_eq!(msg, Some(text::color_macros(&g.cfg.entry_deny_message)));
        // A second refusal within two seconds is silent but still a refusal.
        assert_eq!(check_step(&g, &pu(21, 20.5), [19.5, 64.0, 5.0], &fill), Some(String::new()));
        // Standing inside is refused too (WorldGuard checks the destination, not the crossing).
        assert_eq!(check_step(&g, &pu(22, 21.5), [20.5, 64.0, 5.0], &fill), Some(text::color_macros(&g.cfg.entry_deny_message)));
        // Within the same block, into town (entry allowed), and out of town: let through.
        assert_eq!(check_step(&g, &p(19.9), [19.5, 64.0, 5.0], &fill), None);
        assert_eq!(check_step(&g, &p(1.0), [-1.0, 64.0, 5.0], &fill), None);
        assert_eq!(check_step(&g, &p(11.5), [9.5, 64.0, 5.0], &fill), None);
    }

    #[test]
    fn a_step_out_of_a_no_exit_region_is_refused_unless_bypassing() {
        let mut g = guard();
        g.world_mut("minecraft:overworld")
            .get_mut("town")
            .unwrap()
            .set_flag("exit", Some(FlagValue::State(false)));
        let msg = check_step(&g, &pu(23, 11.5), [9.5, 64.0, 5.0], &fill);
        assert_eq!(msg, Some(text::color_macros(&g.cfg.exit_deny_message)));
        // Moves inside the region stay free.
        assert_eq!(check_step(&g, &p(5.5), [4.5, 64.0, 5.0], &fill), None);
        // The tick check (teleports, missed moves) gives the same answer.
        let mut t = Tracker::default();
        t.update(&g, &pu(24, 9.5), 0.0, 0.0, false, &fill);
        let ev = t.update(&g, &pu(24, 11.5), 0.0, 0.0, false, &fill);
        assert!(matches!(ev.as_slice(), [MoveEvent::Tell(_), MoveEvent::PushBack { .. }]));
    }

    #[test]
    fn game_mode_flag_and_restore() {
        let mut g = guard();
        g.world_mut("minecraft:overworld")
            .get_mut("town")
            .unwrap()
            .set_flag("game-mode", Some(FlagValue::Str("ADVENTURE".into())));
        let mut t = Tracker::default();
        t.update(&g, &p(-5.0), 0.0, 0.0, false, &fill);
        let ev = t.update(&g, &p(1.0), 0.0, 0.0, false, &fill);
        assert!(ev.contains(&MoveEvent::GameMode(2)));
        let mut inside = p(15.0);
        inside.game_mode = 2;
        let ev = t.update(&g, &inside, 0.0, 0.0, false, &fill);
        assert!(ev.contains(&MoveEvent::GameMode(0)));
    }
}
