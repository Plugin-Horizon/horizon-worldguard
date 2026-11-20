//! WorldGuard's blacklist: `<data-folder>/worlds/<world>/blacklist.txt`. The file format, the
//! per-entry actions and the check order follow `Blacklist`, `BlacklistEntry` and the action
//! classes of WorldGuard 7. No engine types: the caller turns hooks into [`Event`]s and acts on the
//! [`Verdict`].

use std::collections::{BTreeMap, HashMap};

/// What a blacklist rule is about (`on-break`, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EventType {
    Break,
    Place,
    Interact,
    Dispense,
    DestroyWith,
    Acquire,
    Equip,
    Drop,
    Use,
}

impl EventType {
    pub const ALL: [EventType; 9] = [
        EventType::Break,
        EventType::Place,
        EventType::Interact,
        EventType::Dispense,
        EventType::DestroyWith,
        EventType::Acquire,
        EventType::Equip,
        EventType::Drop,
        EventType::Use,
    ];

    /// The key in `blacklist.txt`.
    pub fn rule(self) -> &'static str {
        match self {
            EventType::Break => "on-break",
            EventType::Place => "on-place",
            EventType::Interact => "on-interact",
            EventType::Dispense => "on-dispense",
            EventType::DestroyWith => "on-destroy-with",
            EventType::Acquire => "on-acquire",
            EventType::Equip => "on-equip",
            EventType::Drop => "on-drop",
            EventType::Use => "on-use",
        }
    }

    /// `getDescription()`: the verb in "<player> tried to <verb> <target>".
    pub fn description(self) -> &'static str {
        match self {
            EventType::Break => "break",
            EventType::Place => "place",
            EventType::Interact => "interact with",
            EventType::Dispense => "dispense",
            EventType::DestroyWith => "destroy with",
            EventType::Acquire => "acquire",
            EventType::Equip => "equip",
            EventType::Drop => "drop",
            EventType::Use => "use",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
    Ban,
    Kick,
    Log,
    Notify,
    Tell,
}

impl Action {
    fn named(name: &str) -> Option<Action> {
        Some(match name.to_ascii_lowercase().as_str() {
            "allow" => Action::Allow,
            "deny" => Action::Deny,
            "ban" => Action::Ban,
            "kick" => Action::Kick,
            "log" => Action::Log,
            "notify" => Action::Notify,
            "tell" => Action::Tell,
            _ => return None,
        })
    }
}

/// One `[target]` heading's rules (WorldGuard builds one entry per target in the heading).
#[derive(Clone, Debug, Default)]
struct Entry {
    ignore_groups: Vec<String>,
    ignore_perms: Vec<String>,
    message: Option<String>,
    comment: Option<String>,
    actions: BTreeMap<EventType, Vec<Action>>,
}

/// A block or item id without namespace, lower case: `minecraft:Stone` -> `stone`.
pub fn norm(id: &str) -> String {
    let l = id.trim().to_ascii_lowercase();
    l.strip_prefix("minecraft:").unwrap_or(&l).to_owned()
}

/// `Stone`-style display name of an id (`oak_planks` -> `Oak Planks`).
pub fn friendly(id: &str) -> String {
    norm(id)
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What happened, as the check sees it.
#[derive(Clone, Debug)]
pub struct Event {
    pub kind: EventType,
    /// The player's name; None for a dispenser.
    pub player: Option<String>,
    /// Block or item id of the target.
    pub target: String,
}

/// What the caller must do after a check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verdict {
    pub allowed: bool,
    /// Lines for the player (`tell`).
    pub tell: Vec<String>,
    /// `notify` lines for everyone with `worldguard.notify`.
    pub notify: Vec<String>,
    /// `log` lines for the console.
    pub log: Vec<String>,
    pub kick: Option<String>,
    pub ban: Option<String>,
}

#[derive(Default)]
pub struct Blacklist {
    pub whitelist: bool,
    /// target id -> entries
    index: HashMap<String, Vec<Entry>>,
    /// cause name -> (event type, time in ms) of the last event (`repeatingEventCache`, 30 s).
    repeat: HashMap<String, (EventType, u64, u64)>,
}

fn strip_trailing_empty(mut parts: Vec<&str>) -> Vec<&str> {
    // Java's `String.split` drops trailing empty strings.
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    parts
}

impl Blacklist {
    /// Reads a `blacklist.txt`. The second value is the list of warnings WorldGuard logs.
    pub fn parse(text: &str, whitelist: bool) -> (Blacklist, Vec<String>) {
        let mut bl = Blacklist { whitelist, ..Blacklist::default() };
        let mut warnings = Vec::new();
        let mut current: Vec<(String, Entry)> = Vec::new();
        let mut have_heading = false;
        let flush = |bl: &mut Blacklist, current: &mut Vec<(String, Entry)>| {
            for (target, entry) in current.drain(..) {
                bl.index.entry(target).or_default().push(entry);
            }
        };
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
                flush(&mut bl, &mut current);
                have_heading = true;
                for item in line[1..line.len() - 1].split(',') {
                    let id = norm(item);
                    if id.is_empty() {
                        warnings.push(format!("Could not parse a block/item heading: Unknown block or item name: {id}"));
                        continue;
                    }
                    current.push((id, Entry::default()));
                }
                continue;
            }
            if !have_heading {
                warnings.push(format!("Found option with no heading for '{line}'"));
                continue;
            }
            let parts = strip_trailing_empty(line.split('=').collect());
            if parts.len() <= 1 {
                warnings.push(format!("Found option with no value for '{line}'"));
                continue;
            }
            let (key, value) = (parts[0], parts[1]);
            let mut unknown = false;
            for (_, entry) in current.iter_mut() {
                if key.eq_ignore_ascii_case("ignore-groups") {
                    entry.ignore_groups = value.split(',').map(|g| g.to_ascii_lowercase()).collect();
                } else if key.eq_ignore_ascii_case("ignore-perms") {
                    entry.ignore_perms = value.split(',').map(str::to_owned).collect();
                } else if key.eq_ignore_ascii_case("message") {
                    entry.message = Some(value.trim().to_owned());
                } else if key.eq_ignore_ascii_case("comment") {
                    entry.comment = Some(value.trim().to_owned());
                } else if let Some(t) = EventType::ALL.iter().find(|t| t.rule().eq_ignore_ascii_case(key)) {
                    let list = entry.actions.entry(*t).or_default();
                    for name in value.split(',') {
                        match Action::named(name.trim()) {
                            Some(a) => list.push(a),
                            None => warnings.push(format!("Unknown blacklist action: {}", name.trim())),
                        }
                    }
                } else {
                    unknown = true;
                }
            }
            if unknown {
                warnings.push(format!("Unknown option '{key}' for '{line}'"));
            }
        }
        flush(&mut bl, &mut current);
        (bl, warnings)
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Whether any entry names `target` (a cheap test before the caller reads the player).
    pub fn has_target(&self, target: &str) -> bool {
        self.index.contains_key(&norm(target))
    }

    /// `Blacklist.check`. `ignores` answers `ignore-groups` / `ignore-perms` for the player
    /// (true = skip this entry); `now_ms` is a monotonic clock in milliseconds.
    pub fn check(
        &mut self,
        event: &Event,
        force_repeat: bool,
        silent: bool,
        now_ms: u64,
        ignores: &mut dyn FnMut(&[String], &[String]) -> bool,
    ) -> Verdict {
        let id = norm(&event.target);
        let mut verdict = Verdict { allowed: true, ..Verdict::default() };
        let Some(entries) = self.index.get(&id).cloned() else {
            return verdict;
        };
        let cause = event.player.clone().unwrap_or_else(|| "a dispenser".to_owned());
        let name = friendly(&id);
        for entry in &entries {
            // BlacklistEntry.check
            if event.player.is_some() && ignores(&entry.ignore_groups, &entry.ignore_perms) {
                continue;
            }
            self.repeat.retain(|_, (_, _, seen)| *seen + 30_000 > now_ms);
            let repeating = self
                .repeat
                .get(&cause)
                .is_some_and(|(t, time, _)| *t == event.kind && *time + 3000 > now_ms);
            if repeating {
                if let Some(v) = self.repeat.get_mut(&cause) {
                    v.2 = now_ms;
                }
            } else {
                self.repeat.insert(cause.clone(), (event.kind, now_ms, now_ms));
            }
            let mut ret = !self.whitelist;
            for action in entry.actions.get(&event.kind).map(Vec::as_slice).unwrap_or(&[]) {
                match action {
                    Action::Allow => ret = true,
                    Action::Deny => {
                        ret = false;
                        if silent {
                            // DENY_OVERRIDE: nothing after it counts.
                            break;
                        }
                    }
                    Action::Tell if !silent && (!repeating || force_repeat) => {
                        if event.player.is_some() {
                            verdict.tell.push(match &entry.message {
                                Some(m) => m
                                    .replace("\\n", "\n")
                                    .replace("%s", &name),
                                None => format!("You're not allowed to {} {name}.", event.kind.description()),
                            });
                        }
                    }
                    Action::Notify if !silent && (!repeating || force_repeat) => {
                        let mut line = format!("WG: {cause} ({}) {name}", event.kind.description());
                        if let Some(c) = &entry.comment {
                            line.push_str(&format!(" ({c})"));
                        }
                        verdict.notify.push(line);
                    }
                    Action::Log if !silent && (!repeating || force_repeat) => {
                        let mut line = format!("{cause} tried to {} {name}", event.kind.description());
                        if let Some(c) = &entry.comment {
                            line.push_str(&format!(" ({c})"));
                        }
                        verdict.log.push(line);
                    }
                    Action::Kick if !silent && event.player.is_some() => {
                        verdict.kick = Some(match &entry.message {
                            Some(m) => m.replace("%s", &name),
                            None => format!("You can't {} {name}", event.kind.description()),
                        });
                    }
                    Action::Ban if !silent && event.player.is_some() => {
                        verdict.ban = Some(match &entry.message {
                            Some(m) => format!("Banned: {}", m.replace("%s", &name)),
                            None => format!("Banned: You can't {} {name}", event.kind.description()),
                        });
                    }
                    _ => {}
                }
            }
            if !ret {
                verdict.allowed = false;
            }
        }
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# comment\n[tnt, minecraft:lava_bucket]\non-place=deny,tell,notify\nmessage=No %s here\ncomment=griefing\nignore-groups=Admin\n\n[bedrock]\non-break=deny\non-use=log\n";

    fn ev(kind: EventType, target: &str) -> Event {
        Event { kind, player: Some("Alex".into()), target: target.into() }
    }

    fn no_ignore() -> impl FnMut(&[String], &[String]) -> bool {
        |_, _| false
    }

    #[test]
    fn parses_headings_and_options() {
        let (bl, warnings) = Blacklist::parse(SAMPLE, false);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(bl.has_target("TNT"));
        assert!(bl.has_target("minecraft:lava_bucket"));
        assert!(bl.has_target("bedrock"));
        assert!(!bl.has_target("stone"));
    }

    #[test]
    fn deny_tell_and_notify() {
        let (mut bl, _) = Blacklist::parse(SAMPLE, false);
        let v = bl.check(&ev(EventType::Place, "minecraft:tnt"), false, false, 1000, &mut no_ignore());
        assert!(!v.allowed);
        assert_eq!(v.tell, vec!["No Tnt here".to_owned()]);
        assert_eq!(v.notify, vec!["WG: Alex (place) Tnt (griefing)".to_owned()]);
        // The same event type within 3 s is a repeat: still denied, no second message.
        let v = bl.check(&ev(EventType::Place, "tnt"), false, false, 2000, &mut no_ignore());
        assert!(!v.allowed);
        assert!(v.tell.is_empty() && v.notify.is_empty());
        // Forced repeats talk again.
        let v = bl.check(&ev(EventType::Place, "tnt"), true, false, 2500, &mut no_ignore());
        assert_eq!(v.tell.len(), 1);
    }

    #[test]
    fn other_events_and_targets_pass() {
        let (mut bl, _) = Blacklist::parse(SAMPLE, false);
        assert!(bl.check(&ev(EventType::Break, "tnt"), false, false, 0, &mut no_ignore()).allowed);
        assert!(bl.check(&ev(EventType::Place, "dirt"), false, false, 0, &mut no_ignore()).allowed);
        let v = bl.check(&ev(EventType::Break, "bedrock"), false, false, 0, &mut no_ignore());
        assert!(!v.allowed);
        assert_eq!(v.tell, Vec::<String>::new());
    }

    #[test]
    fn ignore_skips_the_entry() {
        let (mut bl, _) = Blacklist::parse(SAMPLE, false);
        let mut admin = |groups: &[String], _: &[String]| groups.iter().any(|g| g == "admin");
        assert!(bl.check(&ev(EventType::Place, "tnt"), false, false, 0, &mut admin).allowed);
    }

    #[test]
    fn silent_deny_and_whitelist() {
        let (mut bl, _) = Blacklist::parse("[stone]\non-acquire=deny\n", false);
        let v = bl.check(&ev(EventType::Acquire, "stone"), false, true, 0, &mut no_ignore());
        assert!(!v.allowed && v.tell.is_empty());
        // As a whitelist an entry denies unless an action allows.
        let (mut wl, _) = Blacklist::parse("[stone]\non-place=allow\n[dirt]\non-place=tell\n", true);
        assert!(wl.check(&ev(EventType::Place, "stone"), false, false, 0, &mut no_ignore()).allowed);
        assert!(!wl.check(&ev(EventType::Place, "dirt"), false, false, 0, &mut no_ignore()).allowed);
        assert!(wl.check(&ev(EventType::Place, "glass"), false, false, 0, &mut no_ignore()).allowed);
    }

    #[test]
    fn warnings_for_bad_lines() {
        let (_, w) = Blacklist::parse("on-break=deny\n[stone]\non-break\nnonsense=1\non-place=explode\n", false);
        assert_eq!(w.len(), 4, "{w:?}");
    }

    #[test]
    fn kick_uses_the_message() {
        let (mut bl, _) = Blacklist::parse("[tnt]\non-use=kick\nmessage=Do not use %s\n", false);
        let v = bl.check(&ev(EventType::Use, "tnt"), false, false, 0, &mut no_ignore());
        assert!(v.allowed);
        assert_eq!(v.kick.as_deref(), Some("Do not use Tnt"));
    }
}
