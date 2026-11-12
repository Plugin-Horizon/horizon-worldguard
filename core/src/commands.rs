//! `/region` (`/rg`) subcommands: the syntax, aliases, permission checks, replies and edge cases of
//! WorldGuard's `RegionCommands` and `MemberCommands`. Running a command changes the [`Guard`]'s
//! regions and returns lines to send plus effects for the trait mod to carry out (teleports, file
//! loads and saves).
//!
//! Replies are `&`-coded lines. `print` lines are light purple (`Actor.print`), `debug` lines grey
//! (`printDebug`), errors red (`CommandException`). The boxes (`MessageBox`, `PaginationBox`) are
//! WorldEdit components that are not part of WorldGuard's source; their layout here is an
//! approximation. Clickable parts (`[Info]`, `[TP]`, flag choices) are plain text.

use super::flags::{self, FlagValue, Group, Location};
use super::geom::{BlockPos, Shape};
use super::guard::{Guard, Player};
use super::query::{Actor, Query};
use super::region::{Domain, GLOBAL_ID, Region, Subject, uuid_string};
use super::selection::{Selection, Selections};
use super::store::{RemoveMode, World, valid_id};
use std::collections::HashMap;

/// Something the trait mod does after a command.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Teleport { player: u128, region: String, dimension: String, pos: [f64; 3], yaw: f32, pitch: f32 },
    /// Write the region file of this world now.
    Save(String),
    /// Read the region file of this world again.
    Load(String),
}

#[derive(Debug, Default, PartialEq)]
pub struct Reply {
    /// Lines with `&` colour codes.
    pub lines: Vec<String>,
    pub effects: Vec<Effect>,
}

impl Reply {
    /// `Actor.print`: light purple.
    fn print(&mut self, s: impl Into<String>) {
        self.lines.push(format!("&d{}", s.into()));
    }
    /// `Actor.printDebug`: grey.
    fn debug(&mut self, s: impl Into<String>) {
        self.lines.push(format!("&7{}", s.into()));
    }
    fn err(&mut self, s: impl Into<String>) {
        self.lines.push(format!("&c{}", s.into()));
    }
    fn raw(&mut self, s: impl Into<String>) {
        self.lines.push(s.into());
    }
}

/// What a command may look up about players.
pub trait Players {
    /// A player name (online or known) to its UUID and spelling.
    fn resolve(&self, name: &str) -> Option<(u128, String)>;
    /// The last known name of a UUID.
    fn name_of(&self, uuid: u128) -> Option<String>;
    /// Whether the player holds a permission node (operators pass by level, see the host).
    fn has_permission(&self, uuid: u128, node: &str) -> bool;
}

pub struct Ctx<'a> {
    /// None for the console.
    pub sender: Option<&'a Player>,
    pub selections: &'a mut dyn Selections,
    pub players: &'a dyn Players,
    /// The world's height range, for `migrateheights` (`(min_y, max_y)` inclusive).
    pub height: (i32, i32),
}

/// Parsed options and positional words.
struct Args {
    opts: HashMap<char, Option<String>>,
    words: Vec<String>,
}

impl Args {
    fn has(&self, c: char) -> bool {
        self.opts.contains_key(&c)
    }
    fn val(&self, c: char) -> Option<&str> {
        self.opts.get(&c).and_then(|v| v.as_deref())
    }
}

/// Splits a command line into words; a quoted part (`"my world"`) is one word.
fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Splits `rest` into options and words. `valued` options take the next word; `bools` don't.
/// A word like `-5` (a number) is a word, not an option.
fn parse_args(rest: &[String], valued: &str, bools: &str) -> Result<Args, String> {
    let mut opts = HashMap::new();
    let mut words = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let w = rest[i].as_str();
        let opt = w
            .strip_prefix('-')
            .filter(|o| !o.is_empty() && o.chars().all(|c| c.is_ascii_alphabetic()));
        if let Some(o) = opt {
            let mut consumed_value = false;
            for c in o.chars() {
                if valued.contains(c) {
                    let v = rest.get(i + 1).ok_or(format!("Option -{c} needs a value."))?;
                    opts.insert(c, Some(v.clone()));
                    consumed_value = true;
                } else if bools.contains(c) {
                    opts.insert(c, None);
                } else {
                    return Err(format!("Unknown option -{c}."));
                }
            }
            i += if consumed_value { 2 } else { 1 };
            continue;
        }
        words.push(w.to_owned());
        i += 1;
    }
    Ok(Args { opts, words })
}

/// (name, usage, min words, max words) as the commands declare them.
const SPECS: &[(&str, &str, usize, usize)] = &[
    ("define", "[-w <world>] <id> [<owner1> [<owner2> [<owners...>]]]", 1, usize::MAX),
    ("redefine", "[-w <world>] <id>", 1, 1),
    ("claim", "<id>", 1, 1),
    ("select", "[-w <world>] [id]", 0, 1),
    ("info", "[id]", 0, 1),
    ("list", "[-w world] [-p owner [-n]] [-s] [-i filter] [page]", 0, 1),
    ("flag", "<id> <flag> [-w world] [-g group] [value]", 2, usize::MAX),
    ("flags", "[-p <page>] [id]", 0, 2),
    ("setpriority", "<id> <priority>", 2, 2),
    ("setparent", "<id> [parent-id]", 1, 2),
    ("remove", "<id>", 1, 1),
    ("addmember", "<id> <members...>", 2, usize::MAX),
    ("addowner", "<id> <owners...>", 2, usize::MAX),
    ("removemember", "<id> <owners...>", 1, usize::MAX),
    ("removeowner", "<id> <owners...>", 1, usize::MAX),
    ("teleport", "[-w world] [-c|s] <id>", 1, 1),
    ("load", "[world]", 0, usize::MAX),
    ("save", "[world]", 0, usize::MAX),
    ("bypass", "[on|off]", 0, usize::MAX),
    ("migrateheights", "[world]", 0, 1),
    ("migratedb", "<from> <to>", 2, 2),
    ("migrateuuid", "", 0, 0),
];

/// The `/rg` usages the client completes, without the root name (`register_commands` adds the
/// root in front of each). Every usage is a path `run` dispatches. Fixed choices are literals
/// (`on`, `off`, `-w`, `-g`, the flag names from `flags::FLAGS`); the group after `-g` is a word,
/// since the six groups as literals would multiply the tree. Options come before the positional
/// words, as literals (a usage cannot put a literal after an optional argument).
pub fn usages() -> Vec<String> {
    let mut out: Vec<String> = [
        "help",
        "define <id:word> [owners:text]",
        "define -g <id:word> [owners:text]",
        "define -n <id:word> [owners:text]",
        "define -w <world:word> <id:word> [owners:text]",
        "redefine <id:word>",
        "redefine -g <id:word>",
        "redefine -w <world:word> <id:word>",
        "claim <id:word>",
        "select [id:word]",
        "select -w <world:word> [id:word]",
        "info [id:word]",
        "info -u [id:word]",
        "info -s [id:word]",
        "info -w <world:word> [id:word]",
        "list [page:integer]",
        "list -s [page:integer]",
        "list -i <filter:word> [page:integer]",
        "list -p <player:player_name> [page:integer]",
        "list -p <player:player_name> -n [page:integer]",
        "list -w <world:word> [page:integer]",
        "flags [id:word]",
        "flags -p <page:integer> [id:word]",
        "flags -w <world:word> [id:word]",
        "setpriority <id:word> <priority:integer>",
        "setpriority -w <world:word> <id:word> <priority:integer>",
        "setparent <id:word> [parent:word]",
        "setparent -w <world:word> <id:word> [parent:word]",
        "remove <id:word>",
        "remove -f <id:word>",
        "remove -u <id:word>",
        "remove -w <world:word> <id:word>",
        "addmember <id:word> <player:player_name> [others:text]",
        "addmember -n <id:word> <names:text>",
        "addmember -w <world:word> <id:word> <player:player_name> [others:text]",
        "addowner <id:word> <player:player_name> [others:text]",
        "addowner -n <id:word> <names:text>",
        "addowner -w <world:word> <id:word> <player:player_name> [others:text]",
        "removemember <id:word> [player:player_name] [others:text]",
        "removemember <id:word> -a",
        "removemember -w <world:word> <id:word> [player:player_name]",
        "removeowner <id:word> [player:player_name] [others:text]",
        "removeowner <id:word> -a",
        "removeowner -w <world:word> <id:word> [player:player_name]",
        "teleport <id:word>",
        "teleport -s <id:word>",
        "teleport -c <id:word>",
        "teleport -s -c <id:word>",
        "teleport -w <world:word> <id:word>",
        "load",
        "load -w <world:word>",
        "save",
        "save -w <world:word>",
        "bypass",
        "bypass on",
        "bypass off",
        "migrateheights",
        "migrateheights -y",
        "migrateheights -y -w <world:word>",
        "migratedb <from:word> <to:word>",
        "migratedb -y <from:word> <to:word>",
        "migrateuuid",
    ]
    .iter()
    .map(|u| (*u).to_owned())
    .collect();
    for def in flags::FLAGS {
        let n = def.name;
        out.push(format!("flag <id:word> {n} [value:text]"));
        out.push(format!("flag <id:word> {n} -e"));
        out.push(format!("flag <id:word> {n} -g <group:word> [value:text]"));
    }
    out
}

fn canonical(sub: &str) -> Option<&'static str> {
    Some(match sub.to_ascii_lowercase().as_str() {
        "define" | "def" | "d" | "create" => "define",
        "redefine" | "update" | "move" => "redefine",
        "claim" => "claim",
        "select" | "sel" | "s" => "select",
        "info" | "i" => "info",
        "list" => "list",
        "flags" => "flags",
        "flag" | "f" => "flag",
        "setpriority" | "priority" | "pri" => "setpriority",
        "setparent" | "parent" | "par" => "setparent",
        "remove" | "delete" | "del" | "rem" => "remove",
        "addmember" | "addmem" | "am" => "addmember",
        "addowner" | "ao" => "addowner",
        "removemember" | "remmember" | "removemem" | "remmem" | "rm" => "removemember",
        "removeowner" | "remowner" | "ro" => "removeowner",
        "teleport" | "tp" => "teleport",
        "load" | "reload" => "load",
        "save" | "write" => "save",
        "toggle-bypass" | "bypass" => "bypass",
        "migratedb" => "migratedb",
        "migrateuuid" => "migrateuuid",
        "migrateheights" => "migrateheights",
        "help" | "?" => "help",
        _ => return None,
    })
}

/// The flags a command takes: (value options, switches).
fn option_spec(cmd: &str) -> (&'static str, &'static str) {
    match cmd {
        "define" => ("w", "ng"),
        "redefine" => ("w", "g"),
        "select" => ("w", ""),
        "info" => ("w", "us"),
        "list" => ("wpi", "ns"),
        "flag" => ("gwh", "e"),
        "flags" => ("pw", ""),
        "setpriority" | "setparent" => ("w", ""),
        "remove" => ("w", "fu"),
        "addmember" | "addowner" => ("w", "n"),
        "removemember" | "removeowner" => ("w", "na"),
        "teleport" => ("w", "cs"),
        "load" | "save" => ("w", ""),
        "migrateheights" => ("w", "y"),
        "migratedb" => ("", "y"),
        _ => ("", ""),
    }
}

const NO_PERMISSION: &str = "You don't have permission to do that.";

/// Runs `/rg <line>`.
pub fn run(guard: &mut Guard, ctx: &mut Ctx<'_>, line: &str) -> Reply {
    let mut r = Reply::default();
    let parts = tokenize(line);
    let Some(first) = parts.first() else {
        help(&mut r);
        return r;
    };
    let Some(cmd) = canonical(first) else {
        r.err(format!("Unknown subcommand '{first}'."));
        help(&mut r);
        return r;
    };
    if cmd == "help" {
        help(&mut r);
        return r;
    }
    let rest = &parts[1..];
    let (valued, bools) = option_spec(cmd);
    let args = match parse_args(rest, valued, bools) {
        Ok(a) => a,
        Err(e) => {
            r.err(e);
            return r;
        }
    };
    if let Some((_, usage, min, max)) = SPECS.iter().find(|s| s.0 == cmd) {
        let n = args.words.len();
        if n < *min || n > *max {
            r.err(if n < *min { "Too few arguments." } else { "Too many arguments." });
            r.err(format!("Usage: /rg {} {usage}", first.to_ascii_lowercase()));
            return r;
        }
    }
    let res = match cmd {
        "define" => define(guard, ctx, &args, &mut r, false),
        "redefine" => define(guard, ctx, &args, &mut r, true),
        "claim" => claim(guard, ctx, &args, &mut r),
        "select" => select(guard, ctx, &args, &mut r),
        "info" => info(guard, ctx, &args, &mut r),
        "list" => list(guard, ctx, &args, &mut r),
        "flags" => flag_helper(guard, ctx, &args, &mut r),
        "flag" => set_flag(guard, ctx, &args, &mut r),
        "setpriority" => set_priority(guard, ctx, &args, &mut r),
        "setparent" => set_parent(guard, ctx, &args, &mut r),
        "remove" => remove(guard, ctx, &args, &mut r),
        "addmember" | "addowner" => add_people(guard, ctx, &args, &mut r, cmd == "addowner"),
        "removemember" | "removeowner" => remove_people(guard, ctx, &args, &mut r, cmd == "removeowner"),
        "teleport" => teleport(guard, ctx, &args, &mut r),
        "load" | "save" => load_save(guard, ctx, &args, &mut r, cmd == "load"),
        "bypass" => bypass(guard, ctx, &args, &mut r),
        "migrateheights" => migrate_heights(guard, ctx, &args, &mut r),
        "migratedb" => migrate_db(guard, ctx, &args, &mut r),
        "migrateuuid" => migrate_uuid(guard, ctx, &mut r),
        _ => Err("Unknown subcommand.".into()),
    };
    if let Err(e) = res {
        for l in e.split('\n') {
            r.err(l);
        }
    }
    r
}

fn help(r: &mut Reply) {
    r.print("/region (/rg) subcommands:");
    for (name, usage, _, _) in SPECS {
        let name = if *name == "bypass" { "toggle-bypass" } else { name };
        r.debug(format!("  /rg {name} {usage}").trim_end().to_owned());
    }
}

type Res = Result<(), String>;

// -- checks (RegionCommandsBase) ----------------------------------------------------------------

fn is_admin(guard: &Guard, ctx: &Ctx<'_>) -> bool {
    ctx.sender.is_none_or(|p| guard.is_admin(p))
}

/// `hasPluginPermission`: a plain node `worldguard.<node>`.
fn plugin_perm(guard: &Guard, ctx: &Ctx<'_>, node: &str, cmd: &str) -> bool {
    let Some(p) = ctx.sender else { return true };
    guard.is_admin(p)
        || ctx.players.has_permission(p.subject.uuid, &format!("worldguard.{node}"))
        || guard.cfg.player_commands.iter().any(|c| c == cmd)
}

/// `hasPatternPermission` of `RegionPermissionModel`: `worldguard.region.<perm>.<id>`, with the
/// `.own.` and `.member.` forms for the region's owners and members. `player-commands` lets
/// the owners of a region run the listed commands on it.
fn pattern_perm(guard: &Guard, ctx: &Ctx<'_>, w: &World, region: &Region, perm: &str, cmd: &str) -> bool {
    let Some(p) = ctx.sender else { return true };
    if guard.is_admin(p) {
        return true;
    }
    let id = region.id.to_ascii_lowercase();
    let (member, owner) = Query::new(w).standing(region, &Actor::Player(&p.subject));
    let has = |node: String| ctx.players.has_permission(p.subject.uuid, &node);
    if owner {
        return has(format!("worldguard.region.{perm}.own.{id}"))
            || has(format!("worldguard.region.{perm}.member.{id}"))
            || guard.cfg.player_commands.iter().any(|c| c == cmd);
    }
    if member {
        return has(format!("worldguard.region.{perm}.member.{id}"));
    }
    has(format!("worldguard.region.{perm}.{id}"))
}

/// `checkWorld`: the world folder name a command acts on: `-w`, else the sender's.
fn check_world(guard: &Guard, ctx: &Ctx<'_>, a: &Args, flag: char) -> Result<String, String> {
    if let Some(w) = a.val(flag) {
        return match_world(guard, w);
    }
    match ctx.sender {
        Some(p) => Ok(guard.cfg.world_of(&p.dimension)),
        None => Err(format!("Please specify the world with -{flag} world_name.")),
    }
}

fn match_world(guard: &Guard, w: &str) -> Result<String, String> {
    if let Some(tag) = w.strip_prefix('#') {
        let dim = match tag.to_ascii_lowercase().as_str() {
            "main" | "normal" => "minecraft:overworld",
            "nether" => "minecraft:the_nether",
            "end" => "minecraft:the_end",
            _ => return Err(format!("Invalid identifier '{w}'.")),
        };
        return Ok(guard.cfg.world_of(dim));
    }
    if guard.cfg.worlds.values().any(|n| n == w) {
        return Ok(w.to_owned());
    }
    if w.contains(':') {
        return Ok(guard.cfg.world_of(w));
    }
    Err("No world by that exact name found.".into())
}

fn dimension_of_world(guard: &Guard, world: &str) -> String {
    guard
        .cfg
        .dimension_of(world)
        .unwrap_or_else(|| format!("minecraft:{world}"))
}

/// `checkRegionManager`.
fn check_manager(guard: &Guard) -> Res {
    if !guard.cfg.enable {
        return Err("Region support is disabled in the target world. It can be enabled per-world in WorldGuard's configuration files. However, you may need to restart your server afterwards.".into());
    }
    Ok(())
}

/// `checkRegionId`: the lower-cased id.
fn check_id(id: &str, allow_global: bool) -> Result<String, String> {
    if !valid_id(id) {
        return Err(format!("The region name of '{id}' contains characters that are not allowed."));
    }
    if !allow_global && id.eq_ignore_ascii_case(GLOBAL_ID) {
        return Err("Sorry, you can't use __global__ here.".into());
    }
    Ok(id.to_ascii_lowercase())
}

/// `checkExistingRegion`: the id of a region that exists (the global region is made on demand).
fn check_existing(guard: &mut Guard, world: &str, id: &str, allow_global: bool) -> Result<String, String> {
    let id = check_id(id, allow_global)?;
    let dim = dimension_of_world(guard, world);
    let w = guard.world_mut(&dim);
    if w.get(&id).is_none() {
        if id == GLOBAL_ID {
            w.global_mut();
            return Ok(id);
        }
        return Err(format!("No region could be found with the name of '{id}'."));
    }
    Ok(id)
}

/// `checkRegionStandingIn`: the single region at the player's feet, or an error that lists them
/// (those the player may use `perm` on; the rest are counted as hidden).
fn check_standing_in(
    guard: &mut Guard,
    ctx: &Ctx<'_>,
    world: &str,
    r: &mut Reply,
    allow_global: bool,
    perm: &str,
) -> Result<String, String> {
    let p = ctx.sender.ok_or("A player is expected.")?;
    let ids: Vec<String> = match guard.worlds.get(world) {
        Some(w) => Query::new(w).ids_at(p.block()),
        None => Vec::new(),
    };
    match ids.len() {
        0 => {
            if allow_global {
                let id = check_existing(guard, world, GLOBAL_ID, true)?;
                r.debug("You're not standing in any regions. Using the global region for this world instead.");
                Ok(id)
            } else {
                Err("You're not standing in a region. Specify an ID if you want to select a specific region.".into())
            }
        }
        1 => Ok(ids[0].clone()),
        _ => {
            let g: &Guard = guard;
            let w = g.worlds.get(world);
            let shown: Vec<&String> = ids
                .iter()
                .filter(|id| w.is_some_and(|w| w.get(id).is_some_and(|reg| pattern_perm(g, ctx, w, reg, perm, perm))))
                .collect();
            let hidden = ids.len() - shown.len();
            let mut line = String::from("&6Current regions: ");
            if shown.is_empty() {
                line.push_str(&format!("&7{hidden} hidden regions"));
            } else {
                let names: Vec<String> = shown.iter().map(|s| format!("&b{s}")).collect();
                line.push_str(&names.join("&f, "));
                if hidden > 0 {
                    line.push_str(&format!("&7, and {hidden} hidden regions"));
                }
            }
            r.raw(line);
            Err("You're standing in several regions (please pick one).".into())
        }
    }
}

/// `checkRegionFromSelection`.
fn selection_shape(ctx: &Ctx<'_>, guard: &Guard, world: &str) -> Result<Shape, String> {
    let p = ctx.sender.ok_or("Please select an area first. Use WorldEdit to make a selection! (see: https://worldedit.enginehub.org/en/latest/usage/regions/selections/).")?;
    let sel = ctx.selections.get(p.subject.uuid).ok_or("Please select an area first. Use WorldEdit to make a selection! (see: https://worldedit.enginehub.org/en/latest/usage/regions/selections/).")?;
    if guard.cfg.world_of(sel.dimension()) != world {
        return Err("Please select an area first. Use WorldEdit to make a selection! (see: https://worldedit.enginehub.org/en/latest/usage/regions/selections/).".into());
    }
    sel.shape()
}

/// `(Warning: The height of the region was N block(s).)`
fn warn_about_dimensions(r: &mut Reply, region: &Region) {
    if let Some((lo, hi)) = region.shape.bounds() {
        let height = hi.y - lo.y;
        if height <= 2 {
            r.debug(format!("(Warning: The height of the region was {} block(s).)", height + 1));
        }
    }
}

fn inform_new_user(r: &mut Reply, w: &World, region: &Region) {
    if w.len() <= 2 {
        r.raw(format!(
            "&7(This region is NOW PROTECTED from modification from others. Don't want that? Use &b/rg flag {} passthrough allow&7)",
            region.id
        ));
    }
}

// -- define / redefine / claim --------------------------------------------------------------------

/// Adds one owner or member given as a name, `uuid:`/UUID or `g:group`. `names_only`: store
/// a name even when the player is known (`-n`).
fn add_one(d: &mut Domain, who: &str, players: &dyn Players, names_only: bool) -> bool {
    let bytes = who.as_bytes();
    if bytes.len() > 2 && (bytes[0] == b'g' || bytes[0] == b'G') && bytes[1] == b':' {
        return d.add_group(&who[2..]);
    }
    if let Some(u) = super::region::parse_uuid(who.strip_prefix("uuid:").unwrap_or(who)) {
        return d.add_uuid(u);
    }
    if !names_only
        && let Some((u, _)) = players.resolve(who)
    {
        return d.add_uuid(u);
    }
    d.add_name(who)
}

fn define(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply, re: bool) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    let dim = dimension_of_world(guard, &world);
    if !re && !plugin_perm(guard, ctx, "region.define", "define") {
        return Err(NO_PERMISSION.into());
    }
    let id = check_id(&a.words[0], false)?;
    check_manager(guard)?;
    let existing: Option<Region> = if re {
        let id = check_existing(guard, &world, &id, false)?;
        let w = guard.worlds.get(&world).ok_or("Region data failed to load for this world. Please ask a server administrator to read the logs to identify the reason.")?;
        let reg = w.get(&id).cloned().ok_or(NO_PERMISSION)?;
        if !pattern_perm(guard, ctx, w, &reg, "redefine", "redefine") {
            return Err(NO_PERMISSION.into());
        }
        Some(reg)
    } else {
        if guard.worlds.get(&world).is_some_and(|w| w.get(&id).is_some()) {
            return Err(format!(
                "A region with that name already exists. Please choose another name. To change the shape, use /region redefine {id}."
            ));
        }
        None
    };
    let shape = if a.has('g') { Shape::Global } else { selection_shape(ctx, guard, &world)? };
    let mut reg = Region::new(&id, shape);
    if let Some(old) = existing {
        // copyFrom: owners, members, flags, priority and parent carry over.
        reg.owners = old.owners;
        reg.members = old.members;
        reg.flags = old.flags;
        reg.groups = old.groups;
        reg.extra = old.extra;
        reg.raw_groups = old.raw_groups;
        reg.priority = old.priority;
        reg.parent = old.parent;
    } else if a.words.len() >= 2 {
        for name in &a.words[1..] {
            add_one(&mut reg.owners, name, ctx.players, a.has('n'));
        }
    }
    let w = guard.world_mut(&dim);
    w.put(reg);
    let reg = w.get(&id).cloned().ok_or("Region data failed to load for this world.")?;
    let w = &*w;
    if re {
        r.print(format!("Region '{id}' has been updated with a new area."));
    } else {
        r.print(format!("A new region has been made named '{id}'."));
    }
    warn_about_dimensions(r, &reg);
    inform_new_user(r, w, &reg);
    Ok(())
}

fn claim(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let p = ctx.sender.ok_or("A player is expected.")?;
    if !plugin_perm(guard, ctx, "region.claim", "claim") {
        return Err(NO_PERMISSION.into());
    }
    let id = check_id(&a.words[0], false)?;
    let world = guard.cfg.world_of(&p.dimension);
    check_manager(guard)?;
    let shape = selection_shape(ctx, guard, &world)?;
    let unbounded = is_admin(guard, ctx)
        || ctx.players.has_permission(p.subject.uuid, "worldguard.region.unlimited");
    let me = p.subject.clone();
    let cfg = guard.cfg.clone();
    let dim = dimension_of_world(guard, &world);
    let w = guard.world_mut(&dim);
    if w.get(&id).is_some() {
        return Err(format!(
            "A region with that name already exists. Please choose another name."
        ));
    }
    if !unbounded {
        let max = cfg.max_regions_for(&me.groups);
        let owned = w.all().filter(|x| x.owners.contains(&me)).count() as i64;
        if max >= 0 && owned >= max {
            return Err("You own too many regions, delete one first to claim a new one.".into());
        }
    }
    let q = Query::new(w);
    let actor = Actor::Player(&me);
    let overlaps = w.intersecting(&shape);
    if !overlaps.is_empty() {
        if !overlaps.iter().all(|o| q.standing(o, &actor).1) {
            return Err("This region overlaps with someone else's region.".into());
        }
    } else if cfg.claim_only_inside_existing {
        return Err("You may only claim regions inside existing regions that you or your group own.".into());
    }
    if !unbounded {
        if matches!(shape, Shape::Polygon { .. }) {
            return Err("Polygons are currently not supported for /rg claim.".into());
        }
        if shape.volume() > cfg.max_claim_volume {
            r.err("This region is too large to claim.");
            r.err(format!("Max. volume: {}, your volume: {}", cfg.max_claim_volume, shape.volume()));
            return Ok(());
        }
    }
    let mut reg = Region::new(&id, shape);
    if !cfg.set_parent_on_claim.is_empty() && w.get(&cfg.set_parent_on_claim).is_some() {
        reg.parent = Some(cfg.set_parent_on_claim.to_ascii_lowercase());
    }
    reg.owners.add_uuid(me.uuid);
    w.put(reg);
    r.print(format!("A new region has been claimed named '{id}'."));
    Ok(())
}

// -- select / info / list ---------------------------------------------------------------------------

fn select(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    check_manager(guard)?;
    let id = match a.words.first() {
        None => {
            let p = ctx.sender.ok_or("A player is expected.")?;
            if guard.cfg.world_of(&p.dimension) != world {
                return Err("Please specify a region name.".into());
            }
            check_standing_in(guard, ctx, &world, r, false, "select")?
        }
        Some(id) => check_existing(guard, &world, id, false)?,
    };
    let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
    let reg = w.get(&id).ok_or(NO_PERMISSION)?;
    if !pattern_perm(guard, ctx, w, reg, "select", "select") {
        return Err(NO_PERMISSION.into());
    }
    set_player_selection(guard, ctx, &world, &id, r)
}

/// `setPlayerSelection`.
fn set_player_selection(guard: &Guard, ctx: &mut Ctx<'_>, world: &str, id: &str, r: &mut Reply) -> Res {
    let p = ctx.sender.ok_or("A player is expected.")?;
    let w = guard.worlds.get(world).ok_or(format!("No region could be found with the name of '{id}'."))?;
    let reg = w.get(id).ok_or(format!("No region could be found with the name of '{id}'."))?;
    let Some(sel) = Selection::of_shape(&dimension_of_world(guard, world), &reg.shape) else {
        return Err(format!(
            "Can't select that region! The region type '{}' can't be selected.",
            reg.shape.type_name()
        ));
    };
    if !ctx.selections.set(p.subject.uuid, sel) {
        return Err("Selections need the world editing mod.".into());
    }
    r.print(format!("Region selected as {}", reg.shape.type_name()));
    Ok(())
}

/// The owners or members of a region as `/rg info` shows them (`DefaultDomain`): names, the names
/// of known UUIDs with a `*`, the other UUIDs; groups after a `;`. `uuids` (`-u`) shows UUIDs
/// instead of names. With `colours` (a player) names are yellow and unknown UUIDs are counted.
fn domain_text(d: &Domain, players: &dyn Players, uuids: bool, colours: bool) -> String {
    let mut named: Vec<String> = d.players.clone();
    let mut unknown: Vec<String> = Vec::new();
    for u in &d.unique_ids {
        match if uuids { None } else { super::region::parse_uuid(u).and_then(|x| players.name_of(x)) } {
            Some(n) => named.push(if colours { n } else { format!("{n}*") }),
            None => unknown.push(u.clone()),
        }
    }
    let mut out = String::new();
    if colours {
        named.sort();
        out.push_str(&named.iter().map(|n| format!("&e{n}")).collect::<Vec<_>>().join("&f, "));
        if !unknown.is_empty() {
            if !out.is_empty() {
                out.push_str("&f, ");
            }
            out.push_str(&format!(
                "&7{} unknown uuid{}",
                unknown.len(),
                if unknown.len() == 1 { "" } else { "s" }
            ));
        }
        if !d.groups.is_empty() {
            if !out.is_empty() {
                out.push_str("&f; ");
            }
            out.push_str(&d.groups.iter().map(|g| format!("&7g:&6{g}")).collect::<Vec<_>>().join("&f, "));
        }
        return out;
    }
    let mut all: Vec<String> = named.iter().map(|n| if n.ends_with('*') { n.clone() } else { format!("name:{n}") }).collect();
    all.extend(unknown.iter().map(|u| format!("uuid:{u}")));
    all.sort_by_key(|s| s.to_ascii_lowercase());
    out.push_str(&all.join(", "));
    if !d.groups.is_empty() {
        if !out.is_empty() {
            out.push_str("; ");
        }
        out.push_str(&d.groups.iter().map(|g| format!("g:{g}")).collect::<Vec<_>>().join(", "));
    }
    out
}

/// A `MessageBox`: the title between rules, the lines, a closing rule.
fn message_box(title: &str, body: Vec<String>) -> Vec<String> {
    let mut out = vec![format!("&e&m--------&r &e[ &6{title}&e ] &e&m--------")];
    out.extend(body);
    out.push("&e&m-------------------------------------".to_owned());
    out
}

fn flags_list_text(reg: &Region, colours: bool) -> String {
    let mut items: Vec<String> = Vec::new();
    let named = flags::FLAGS
        .iter()
        .filter_map(|d| reg.flag(d.name).map(|v| (d.name.to_owned(), v, true)))
        .chain(
            reg.flags
                .iter()
                .filter(|(n, _)| flags::lookup_exact(n).is_none())
                .map(|(n, v)| (n.clone(), v, false)),
        );
    for (name, v, builtin) in named {
        let group = reg.group(&name).filter(|_| name != "nonplayer-protection-domains");
        let label = match group {
            Some(g) => format!("{name} -g {}: ", g.file_name()),
            None => format!("{name}: "),
        };
        let danger = matches!(name.as_str(), "build" | "passthrough" | "block-place" | "block-break")
            && !(reg.id == GLOBAL_ID && name == "passthrough");
        if colours {
            let c = if danger {
                "&4"
            } else if builtin {
                "&6"
            } else {
                "&7"
            };
            items.push(format!("{c}{label}&e{}", v.display()));
        } else {
            items.push(format!("&f{label}{}", v.display()));
        }
    }
    if items.is_empty() {
        return if colours { "&c(none)".into() } else { "&f(none)".into() };
    }
    items.join(if colours { "&f, " } else { "&f, " })
}

/// `appendParentTree`: the chain from the root down to `reg`.
fn parent_tree(w: &World, reg: &Region) -> Vec<String> {
    let mut chain: Vec<&Region> = vec![reg];
    chain.extend(w.ancestors(&reg.id));
    chain.reverse();
    let mut out = Vec::new();
    for (indent, cur) in chain.iter().enumerate() {
        let mut line = String::new();
        if indent != 0 {
            line.push_str("&a");
            line.push_str(&" ".repeat(indent));
            line.push('\u{2937}');
        }
        line.push_str(&format!("&a{}", cur.id));
        if cur.id != reg.id {
            line.push_str(&format!("&7 (parent, priority={})", cur.priority));
        }
        out.push(line);
    }
    out
}

fn info(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    check_manager(guard)?;
    let id = match a.words.first() {
        None => {
            if ctx.sender.is_none() {
                return Err("Please specify the region with /region info -w world_name region_name.".into());
            }
            check_standing_in(guard, ctx, &world, r, true, "info")?
        }
        Some(id) => check_existing(guard, &world, id, true)?,
    };
    let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
    let reg = w.get(&id).ok_or(NO_PERMISSION)?;
    if !pattern_perm(guard, ctx, w, reg, "info", "info") {
        return Err(NO_PERMISSION.into());
    }
    if a.has('s') {
        if !pattern_perm(guard, ctx, w, reg, "select", "select") {
            return Err(NO_PERMISSION.into());
        }
        set_player_selection(guard, ctx, &world, &id, r)?;
    }
    let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
    let reg = w.get(&id).ok_or(NO_PERMISSION)?;
    let player = ctx.sender.is_some();
    let uuids = a.has('u');
    let mut body = vec![
        format!(
            "&9Region: &e{}&7 (type=&f{}&7, priority=&f{}&7)",
            reg.id,
            reg.shape.type_name(),
            reg.priority
        ),
        format!("&9Flags: {}", flags_list_text(reg, true)),
    ];
    if reg.parent.is_some() {
        body.extend(parent_tree(w, reg));
    }
    let domain = |d: &Domain| -> String {
        if d.is_empty() {
            "&c(none)".to_owned()
        } else if player {
            domain_text(d, ctx.players, uuids, true)
        } else {
            format!("&e{}", domain_text(d, ctx.players, uuids, false))
        }
    };
    body.push(format!("&9Owners: {}", domain(&reg.owners)));
    body.push(format!("&9Members: {}", domain(&reg.members)));
    if let Some((lo, hi)) = reg.shape.bounds()
        && !matches!(reg.shape, Shape::Global)
    {
        body.push(format!("&9Bounds:&e ({}, {}, {}) -> ({}, {}, {})", lo.x, lo.y, lo.z, hi.x, hi.y, hi.z));
    } else {
        body.push("&9Bounds:&e (0, 0, 0) -> (0, 0, 0)".to_owned());
    }
    if !uuids && !player {
        body.push("&7Any names suffixed by * are 'last seen names' and may not be up to date.".to_owned());
    }
    for l in message_box("Region Info", body) {
        r.raw(l);
    }
    Ok(())
}

/// One entry of `/rg list`.
struct ListEntry {
    id: String,
    owner: bool,
    member: bool,
    lookup: bool,
    tp: Option<&'static str>,
}

fn list(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    let mut page: i64 = match a.words.first() {
        Some(p) => p.parse().map_err(|_| format!("Number expected; string \"{p}\" given."))?,
        None => 1,
    };
    if page < 1 {
        page = 1;
    }
    let mut owned_by: Option<String> = a.val('p').map(str::to_owned);
    if let Some(p) = ctx.sender {
        let may = |target: Option<&str>| match target {
            None => plugin_perm(guard, ctx, "region.list", "list"),
            Some(t) if t.eq_ignore_ascii_case(&p.subject.name) => {
                plugin_perm(guard, ctx, "region.list.own", "list")
            }
            Some(_) => plugin_perm(guard, ctx, "region.list", "list"),
        };
        if !may(owned_by.as_deref()) {
            owned_by = Some(p.subject.name.clone());
            if !may(owned_by.as_deref()) {
                return Err(NO_PERMISSION.into());
            }
        }
    }
    check_manager(guard)?;
    let name_only = a.has('n');
    let sel_shape = if a.has('s') { Some(selection_shape(ctx, guard, &world)?) } else { None };
    let owner_filter: Option<(Subject, bool)> = owned_by.as_deref().map(|name| {
        let (uuid, n) = if name_only {
            (0, name.to_owned())
        } else {
            ctx.players.resolve(name).unwrap_or((0, name.to_owned()))
        };
        (Subject { uuid, name: n, groups: vec![] }, name_only)
    });
    let Some(w) = guard.worlds.get(&world) else {
        for l in message_box("Regions", vec!["&c(none)".into()]) {
            r.raw(l);
        }
        return Ok(());
    };
    let q = Query::new(w);
    let id_filter = a.val('i');
    let me = ctx.sender;
    let mut entries: Vec<ListEntry> = Vec::new();
    let mut global: Option<ListEntry> = None;
    for reg in w.all() {
        if let Some(s) = &sel_shape
            && !reg.is_global()
            && !reg.shape.intersects(s)
        {
            continue;
        }
        if sel_shape.is_some() && reg.is_global() {
            continue;
        }
        if id_filter.is_some_and(|f| !reg.id.contains(f)) {
            continue;
        }
        if reg.is_blank_global() {
            continue;
        }
        let (mut owner, mut member) = (false, false);
        if let Some((s, names_only)) = &owner_filter {
            let in_domain = |d: &Domain| {
                d.players.iter().any(|p| p.eq_ignore_ascii_case(&s.name))
                    || (!names_only && s.uuid != 0 && d.unique_ids.iter().any(|u| u.eq_ignore_ascii_case(&uuid_string(s.uuid))))
            };
            owner = in_domain(&reg.owners);
            member = !owner && in_domain(&reg.members);
            if !owner && !member {
                continue;
            }
        }
        let lookup = me.is_some_and(|_| pattern_perm(guard, ctx, w, reg, "info", "info"));
        let tp = me.and_then(|p| {
            let actor = Actor::Player(&p.subject);
            let has_flag = q.effective_flag(reg, "teleport", &actor).is_some();
            if has_flag && pattern_perm(guard, ctx, w, reg, "teleport", "teleport") {
                Some("[TP]")
            } else if pattern_perm(guard, ctx, w, reg, "teleportcenter", "teleportcenter")
                && !matches!(reg.shape, Shape::Global)
            {
                Some("[TP-Center]")
            } else {
                None
            }
        });
        let e = ListEntry { id: reg.id.clone(), owner, member, lookup, tp };
        if reg.is_global() {
            global = Some(e);
        } else {
            entries.push(e);
        }
    }
    if owner_filter.is_some() {
        entries.sort_by(|a, b| {
            b.owner
                .cmp(&a.owner)
                .then_with(|| b.member.cmp(&a.member))
                .then_with(|| a.id.cmp(&b.id))
        });
    } else {
        entries.sort_by(|a, b| a.id.cmp(&b.id));
    }
    if let Some(g) = global {
        entries.insert(0, g);
    }
    let title = match &owned_by {
        Some(n) => format!("Regions for {n}"),
        None => "Regions".to_owned(),
    };
    let per = 8usize;
    let pages = entries.len().div_ceil(per).max(1);
    if page as usize > pages {
        return Err(format!("Invalid page {page}"));
    }
    let start = (page as usize - 1) * per;
    let mut body = Vec::new();
    for (i, e) in entries.iter().enumerate().skip(start).take(per) {
        let mut line = format!("&d{}.", i + 1);
        if e.owner {
            line.push_str(" &3+");
        } else if e.member {
            line.push_str(" &b-");
        }
        line.push_str(&format!(" &6{}", e.id));
        if e.lookup {
            line.push_str(" &7[Info]");
        }
        if let Some(t) = e.tp {
            line.push_str(&format!(" &7{t}"));
        }
        body.push(line);
    }
    if entries.is_empty() {
        body.push("&c(none)".to_owned());
    }
    for l in message_box(&format!("{title} (page {page}/{pages})"), body) {
        r.raw(l);
    }
    Ok(())
}

// -- flags ------------------------------------------------------------------------------------------

/// The `FlagHelperBox` for a region: one line per flag with the choices of its type.
fn flag_helper_lines(guard: &Guard, ctx: &Ctx<'_>, world: &str, id: &str, page: i64) -> Result<Vec<String>, String> {
    let w = guard.worlds.get(world).ok_or(NO_PERMISSION)?;
    let reg = w.get(id).ok_or(NO_PERMISSION)?;
    let q = Query::new(w);
    let per = 18usize;
    let total = flags::FLAGS.len();
    let pages = total.div_ceil(per).max(1);
    if page < 1 || page as usize > pages {
        return Err(format!("Invalid page {page}"));
    }
    let start = (page as usize - 1) * per;
    let mut body = Vec::new();
    for def in flags::FLAGS.iter().skip(start).take(per) {
        let mut name = def.name.to_owned();
        if def.name == "build" || def.name == "passthrough" {
            name.push('*');
        }
        let pad = ".".repeat(34usize.saturating_sub(name.len()));
        let own = reg.flag(def.name);
        let inherited = if own.is_none() { q.inherited(reg, def.name).map(|(v, _)| v) } else { None };
        let shown = |text: &str, explicit: bool, inh: bool| -> String {
            let c = if explicit { "&f" } else if inh { "&7" } else { "&8" };
            format!("{c}{text}")
        };
        let choices = match def.kind {
            flags::Kind::State => {
                let cur = own.or(inherited).and_then(FlagValue::as_state);
                let inh = own.is_none() && inherited.is_some();
                let s = |want: bool, text: &str| {
                    shown(text, own.is_some() && cur == Some(want), inh && cur == Some(want))
                };
                format!("{} {}", s(true, "Allow"), s(false, "Deny"))
            }
            flags::Kind::Bool => {
                let cur = match own.or(inherited) {
                    Some(FlagValue::Bool(b)) => Some(*b),
                    _ => None,
                };
                let inh = own.is_none() && inherited.is_some();
                let s = |want: bool, text: &str| {
                    shown(text, own.is_some() && cur == Some(want), inh && cur == Some(want))
                };
                format!("{} {}", s(true, "True"), s(false, "False"))
            }
            _ => {
                let text = match own.or(inherited) {
                    Some(FlagValue::Set(items)) => format!("{}x set", items.len()),
                    Some(v) => {
                        let d = v.display().replace('\u{a7}', "&");
                        if d.chars().count() > 23 { format!("{}...", d.chars().take(20).collect::<String>()) } else { d }
                    }
                    None => match def.kind {
                        flags::Kind::Location => "unset location".to_owned(),
                        flags::Kind::Int | flags::Kind::Double => "unset number".to_owned(),
                        flags::Kind::GameMode => "game mode".to_owned(),
                        flags::Kind::Weather => "weather type".to_owned(),
                        flags::Kind::EntitySet => "entity type".to_owned(),
                        _ => "string".to_owned(),
                    },
                };
                shown(&text, own.is_some(), own.is_none() && inherited.is_some())
            }
        };
        let _ = ctx;
        body.push(format!("&6{name} &8{pad} {choices}"));
    }
    let _ = pages;
    Ok(message_box(&format!("Flags for {} (page {page}/{pages})", reg.id), body))
}

fn flag_helper(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    check_manager(guard)?;
    let id = match a.words.first() {
        None => {
            if ctx.sender.is_none() {
                return Err("Please specify the region with /region flags -w world_name region_name.".into());
            }
            check_standing_in(guard, ctx, &world, r, true, "info")?
        }
        Some(id) => check_existing(guard, &world, id, true)?,
    };
    let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
    let reg = w.get(&id).ok_or(NO_PERMISSION)?;
    if !pattern_perm(guard, ctx, w, reg, "info", "info") {
        return Err(NO_PERMISSION.into());
    }
    let page: i64 = match a.val('p') {
        Some(p) => p.parse().map_err(|_| format!("Number expected; string \"{p}\" given."))?,
        None => 1,
    };
    for l in flag_helper_lines(guard, ctx, &world, &id, page)? {
        r.raw(l);
    }
    Ok(())
}

fn set_flag(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    let flag_name = a.words[1].clone();
    let mut value: Option<String> = if a.words.len() >= 3 { Some(a.words[2..].join(" ")) } else { None };
    if a.has('e') {
        if value.is_some() {
            return Err("You cannot use -e(mpty) with a flag value.".into());
        }
        value = Some(String::new());
    }
    check_manager(guard)?;
    let id = check_existing(guard, &world, &a.words[0], true)?;
    {
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let reg = w.get(&id).ok_or(NO_PERMISSION)?;
        if !pattern_perm(guard, ctx, w, reg, "flag.regions", "flag") {
            return Err(NO_PERMISSION.into());
        }
    }
    let Some(def) = flags::lookup(&flag_name) else {
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let reg = w.get(&id).ok_or(NO_PERMISSION)?;
        let mut names: Vec<&str> = flags::FLAGS
            .iter()
            .filter(|d| pattern_perm(guard, ctx, w, reg, &format!("flag.flags.{}", d.name), "flag"))
            .map(|d| d.name)
            .collect();
        names.sort();
        r.err(format!("Unknown flag specified: {flag_name}"));
        let mut line = String::from("&fAvailable flags: ");
        for (i, n) in names.iter().enumerate() {
            line.push_str(if i % 2 == 0 { "&7" } else { "&f" });
            line.push_str(n);
            line.push_str("&f, ");
        }
        if ctx.sender.is_some() {
            line.push_str(&format!("&dOr use the command &b/rg flags {id}"));
        }
        r.raw(line);
        return Ok(());
    };
    if value.is_some() {
        if matches!(def.name, "build" | "block-break" | "block-place") {
            r.raw("&c&lWARNING:&c Setting this flag is not required for protection.".to_owned());
            r.raw("&dSetting this flag will completely override default protection, and apply to members, non-members, pistons, sand physics, and everything else that can modify blocks.".to_owned());
            r.raw("&dOnly set this flag if you are sure you know what you are doing. See &b[this documentation page]&d for more info.".to_owned());
            if ctx.sender.is_none() {
                r.raw("https://worldguard.enginehub.org/en/latest/regions/flags/#protection-related");
            }
        } else if def.name == "passthrough" {
            r.raw("&c&lWARNING:&c This flag is unrelated to moving through regions.".to_owned());
            r.raw("&dIt overrides build checks. If you're unsure what this means, see &b[this documentation page]&d for more info.".to_owned());
            if ctx.sender.is_none() {
                r.raw("https://worldguard.enginehub.org/en/latest/regions/flags/#overrides");
            }
        }
    }
    {
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let reg = w.get(&id).ok_or(NO_PERMISSION)?;
        let sanitized = match &value {
            Some(v) => {
                let s: String = v
                    .trim()
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    .take(20)
                    .collect();
                s
            }
            None => "unset".to_owned(),
        };
        if !pattern_perm(guard, ctx, w, reg, &format!("flag.flags.{}.{sanitized}", def.name), "flag") {
            return Err(NO_PERMISSION.into());
        }
    }
    let group = if a.has('g') {
        if def.name == "nonplayer-protection-domains" {
            return Err(format!("Region flag '{}' does not have a group flag!", def.name));
        }
        let g = a.val('g').unwrap_or("");
        Some(Group::parse(g).ok_or(format!(
            "Unknown value '{g}' in com.sk89q.worldguard.protection.flags.RegionGroup"
        ))?)
    } else {
        None
    };
    let here = ctx.sender.map(|p| Location {
        world: world.clone(),
        pos: p.pos,
        yaw: p.yaw,
        pitch: p.pitch,
    });
    let dim = dimension_of_world(guard, &world);
    let location_inside = guard.cfg.location_flags_only_inside_regions;
    let admin = is_admin(guard, ctx);
    let mut parsed: Option<Option<FlagValue>> = None;
    if let Some(v) = &value {
        let pv = FlagValue::parse_input(def, v, here.as_ref())?;
        if let Some(FlagValue::Location(loc)) = &pv
            && location_inside
            && id != GLOBAL_ID
        {
            let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
            let reg = w.get(&id).ok_or(NO_PERMISSION)?;
            if !reg.contains(BlockPos::of(loc.pos)) {
                let may = ctx.sender.is_none_or(|_| {
                    admin || pattern_perm(guard, ctx, w, reg, "locationoverride", "locationoverride")
                });
                if may {
                    r.debug("WARNING: Flag location is outside of region.");
                } else {
                    return Err("You can't set that flag outside of the region boundaries.".into());
                }
            }
        }
        parsed = Some(pv);
    }
    let helper_page: Option<i64> = match a.val('h') {
        Some(h) => Some(h.parse().map_err(|_| format!("Number expected; string \"{h}\" given."))?),
        None => None,
    };
    let quiet = helper_page.is_some();
    let w = guard.world_mut(&dim);
    let reg = if id == GLOBAL_ID {
        w.global_mut()
    } else {
        w.get_mut(&id).ok_or(format!("No region could be found with the name of '{id}'."))?
    };
    match parsed {
        Some(Some(v)) => {
            let shown = v.display();
            reg.set_flag(def.name, Some(v));
            if !quiet {
                r.print(format!("Region flag {} set on '{id}' to '{shown}'.", def.name));
            }
        }
        Some(None) => {
            reg.set_flag(def.name, None);
            if !quiet {
                r.print(format!(
                    "Region flag {} removed from '{id}'. (Any -g(roups) were also removed.)",
                    def.name
                ));
            }
        }
        None if !a.has('g') => {
            reg.set_flag(def.name, None);
            if !quiet {
                r.print(format!(
                    "Region flag {} removed from '{id}'. (Any -g(roups) were also removed.)",
                    def.name
                ));
            }
        }
        None => {}
    }
    if let Some(g) = group {
        if g == def.default_group {
            reg.set_group(def.name, None);
            r.print(format!("Region group flag for '{}' reset to default.", def.name));
        } else {
            reg.set_group(def.name, Some(g));
            r.print(format!("Region group flag for '{}' set.", def.name));
        }
    }
    let reg_after = reg.clone();
    if let Some(page) = helper_page {
        for l in flag_helper_lines(guard, ctx, &world, &id, page)? {
            r.raw(l);
        }
    } else {
        r.raw(format!(
            "&7(Current flags: {}&7)",
            flags_list_text(&reg_after, false)
        ));
    }
    Ok(())
}

// -- priority, parent, remove ------------------------------------------------------------------------

fn set_priority(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    let pri: i32 = a.words[1]
        .parse()
        .map_err(|_| format!("Number expected; string \"{}\" given.", a.words[1]))?;
    check_manager(guard)?;
    let id = check_existing(guard, &world, &a.words[0], false)?;
    {
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let reg = w.get(&id).ok_or(NO_PERMISSION)?;
        if !pattern_perm(guard, ctx, w, reg, "setpriority", "setpriority") {
            return Err(NO_PERMISSION.into());
        }
    }
    let dim = dimension_of_world(guard, &world);
    let reg = guard.world_mut(&dim).get_mut(&id).ok_or(NO_PERMISSION)?;
    reg.priority = pri;
    r.print(format!("Priority of '{id}' set to {pri} (higher numbers override)."));
    Ok(())
}

fn set_parent(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    check_manager(guard)?;
    let child = check_existing(guard, &world, &a.words[0], false)?;
    let parent = match a.words.get(1) {
        Some(p) => Some(check_existing(guard, &world, p, false)?),
        None => None,
    };
    {
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let c = w.get(&child).ok_or(NO_PERMISSION)?;
        let ok = pattern_perm(guard, ctx, w, c, "setparent", "setparent")
            && parent
                .as_ref()
                .is_none_or(|p| w.get(p).is_some_and(|pr| pattern_perm(guard, ctx, w, pr, "setparent", "setparent")));
        if !ok {
            return Err(NO_PERMISSION.into());
        }
    }
    let dim = dimension_of_world(guard, &world);
    let w = guard.world_mut(&dim);
    if let Err(e) = w.set_parent(&child, parent.as_deref()) {
        if e.contains("circular") {
            let p = parent.clone().unwrap_or_default();
            r.err(format!(
                "Uh oh! Setting '{p}' to be the parent of '{child}' would cause circular inheritance."
            ));
            r.debug(format!("(Current inheritance on '{p}':"));
            if let Some(pr) = w.get(&p) {
                for l in parent_tree(w, pr) {
                    r.raw(l);
                }
            }
            r.debug(")");
            return Ok(());
        }
        return Err(e);
    }
    r.print(format!("Inheritance set for region '{child}'."));
    match parent {
        Some(_) => {
            r.debug("(Current inheritance:");
            if let Some(c) = w.get(&child) {
                for l in parent_tree(w, c) {
                    r.raw(l);
                }
            }
            r.debug(")");
        }
        None => {
            if let Some(last) = r.lines.last_mut() {
                last.push_str("&e Region is now orphaned.");
            }
        }
    }
    Ok(())
}

fn remove(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    let (force, unset) = (a.has('f'), a.has('u'));
    check_manager(guard)?;
    let id = check_existing(guard, &world, &a.words[0], true)?;
    {
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let reg = w.get(&id).ok_or(NO_PERMISSION)?;
        if !pattern_perm(guard, ctx, w, reg, "remove", "remove") {
            return Err(NO_PERMISSION.into());
        }
    }
    if force && unset {
        return Err("You cannot use both -u (unset parent) and -f (remove children) together.".into());
    }
    let mode = if force {
        RemoveMode::Cascade
    } else if unset {
        RemoveMode::Unset
    } else {
        RemoveMode::Refuse
    };
    let dim = dimension_of_world(guard, &world);
    let gone = match guard.world_mut(&dim).remove(&id, mode) {
        Ok(g) => g,
        Err(e) if e.contains("child") => {
            return Err(format!(
                "The region '{id}' has child regions. Use -f to force removal of children or -u to unset the parent value of these children."
            ));
        }
        Err(e) => return Err(e),
    };
    r.print(format!("Successfully removed {}.", gone.join(", ")));
    Ok(())
}

// -- owners and members ---------------------------------------------------------------------------------

fn add_people(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply, owners: bool) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    check_manager(guard)?;
    let id = check_existing(guard, &world, &a.words[0], true)?;
    let (perm, cmd) = if owners { ("addowner", "addowner") } else { ("addmember", "addmember") };
    {
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let reg = w.get(&id).ok_or(NO_PERMISSION)?;
        if !pattern_perm(guard, ctx, w, reg, perm, cmd) {
            return Err(NO_PERMISSION.into());
        }
    }
    let names_only = a.has('n');
    let mut staged = Domain::default();
    for who in &a.words[1..] {
        add_one(&mut staged, who, ctx.players, names_only);
    }
    if owners {
        // checkedAddOwners: the player may not take more regions than allowed, and an
        // unclaimed region needs its own permission.
        if let Some(p) = ctx.sender
            && staged.contains(&p.subject)
            && !ctx.players.has_permission(p.subject.uuid, "worldguard.region.unlimited")
            && !guard.is_admin(p)
        {
            let max = guard.cfg.max_regions_for(&p.subject.groups);
            let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
            let owned = w.all().filter(|x| x.owners.contains(&p.subject)).count() as i64;
            if max >= 0 && owned >= max {
                return Err("You already own the maximum allowed amount of regions.".into());
            }
        }
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let reg = w.get(&id).ok_or(NO_PERMISSION)?;
        if reg.owners.is_empty()
            && !w.ancestors(&id).iter().any(|p| !p.owners.is_empty())
            && let Some(p) = ctx.sender
            && !guard.is_admin(p)
            && !ctx
                .players
                .has_permission(p.subject.uuid, &format!("worldguard.region.addowner.unclaimed.{id}"))
        {
            return Err(NO_PERMISSION.into());
        }
    }
    let dim = dimension_of_world(guard, &world);
    let w = guard.world_mut(&dim);
    let reg = if id == GLOBAL_ID { w.global_mut() } else { w.get_mut(&id).ok_or(NO_PERMISSION)? };
    let d = if owners { &mut reg.owners } else { &mut reg.members };
    for u in &staged.unique_ids {
        if let Some(x) = super::region::parse_uuid(u) {
            d.add_uuid(x);
        }
    }
    for n in &staged.players {
        d.add_name(n);
    }
    for g in &staged.groups {
        d.add_group(g);
    }
    r.print(format!(
        "Region '{id}' updated with new {}.",
        if owners { "owners" } else { "members" }
    ));
    Ok(())
}

fn remove_people(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply, owners: bool) -> Res {
    let world = check_world(guard, ctx, a, 'w')?;
    check_manager(guard)?;
    let id = check_existing(guard, &world, &a.words[0], true)?;
    let (perm, cmd) = if owners { ("removeowner", "removeowner") } else { ("removemember", "removemember") };
    {
        let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
        let reg = w.get(&id).ok_or(NO_PERMISSION)?;
        if !pattern_perm(guard, ctx, w, reg, perm, cmd) {
            return Err(NO_PERMISSION.into());
        }
    }
    if !a.has('a') && a.words.len() < 2 {
        return Err("List some names to remove, or use -a to remove all.".into());
    }
    let dim = dimension_of_world(guard, &world);
    let players = ctx.players;
    let names_only = a.has('n');
    let w = guard.world_mut(&dim);
    let reg = if id == GLOBAL_ID { w.global_mut() } else { w.get_mut(&id).ok_or(NO_PERMISSION)? };
    let d = if owners { &mut reg.owners } else { &mut reg.members };
    if a.has('a') {
        d.clear();
    } else {
        for who in &a.words[1..] {
            let uuid = if names_only { None } else { players.resolve(who).map(|(u, _)| u) };
            d.remove(who, uuid);
        }
    }
    r.print(format!(
        "Region '{id}' updated with {} removed.",
        if owners { "owners" } else { "members" }
    ));
    Ok(())
}

// -- teleport, load/save, bypass, migrations ------------------------------------------------------------

fn teleport(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let p = ctx.sender.ok_or("A player is expected.")?;
    let world = check_world(guard, ctx, a, 'w')?;
    check_manager(guard)?;
    let id = check_existing(guard, &world, &a.words[0], true)?;
    let w = guard.worlds.get(&world).ok_or(NO_PERMISSION)?;
    let reg = w.get(&id).ok_or(NO_PERMISSION)?;
    if !pattern_perm(guard, ctx, w, reg, "teleport", "teleport") {
        return Err(NO_PERMISSION.into());
    }
    let q = Query::new(w);
    let me = Actor::Player(&p.subject);
    let (pos, yaw, pitch, target_world) = if a.has('s') {
        match q.effective_flag(reg, "spawn", &me) {
            Some(FlagValue::Location(l)) => (l.pos, l.yaw, l.pitch, l.world),
            _ => return Err("The region has no spawn point associated.".into()),
        }
    } else if a.has('c') {
        if !pattern_perm(guard, ctx, w, reg, "teleportcenter", "teleportcenter") {
            return Err(NO_PERMISSION.into());
        }
        let Some(c) = reg.shape.center() else {
            return Err("The region has no center point.".into());
        };
        if p.game_mode != 3 {
            return Err("Center teleport is only available in Spectator gamemode.".into());
        }
        (c, 0.0, 0.0, world.clone())
    } else {
        match q.effective_flag(reg, "teleport", &me) {
            Some(FlagValue::Location(l)) => (l.pos, l.yaw, l.pitch, l.world),
            _ => return Err("The region has no teleport point associated.".into()),
        }
    };
    let message = match q.effective_flag(reg, "teleport-message", &me) {
        Some(FlagValue::Str(s)) => s,
        _ => flags::DEFAULT_TELEPORT_MESSAGE.to_owned(),
    };
    let target_world = if target_world.is_empty() { world.clone() } else { target_world };
    let dimension = dimension_of_world(guard, &target_world);
    r.effects.push(Effect::Teleport {
        player: p.subject.uuid,
        region: id.clone(),
        dimension,
        pos,
        yaw,
        pitch,
    });
    r.raw(message.replace("%id%", &id));
    Ok(())
}

fn load_save(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply, load: bool) -> Res {
    let world = check_world(guard, ctx, a, 'w').ok();
    let node = if load { "region.load" } else { "region.save" };
    if !plugin_perm(guard, ctx, node, if load { "load" } else { "save" }) {
        return Err(NO_PERMISSION.into());
    }
    let effect = |w: String| if load { Effect::Load(w) } else { Effect::Save(w) };
    match world {
        Some(w) => {
            check_manager(guard)?;
            r.effects.push(effect(w.clone()));
            r.print(if load {
                format!("Loaded region data for '{w}'")
            } else {
                format!("Saving region data for '{w}'")
            });
        }
        None => {
            let mut worlds: Vec<String> = guard.cfg.worlds.values().cloned().collect();
            for k in guard.worlds.keys() {
                if !worlds.contains(k) {
                    worlds.push(k.clone());
                }
            }
            for w in worlds {
                r.effects.push(effect(w));
            }
            r.print(if load {
                "Successfully load the region data for all worlds."
            } else {
                "Successfully saved the region data for all worlds."
            });
        }
    }
    Ok(())
}

fn bypass(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    let p = ctx.sender.ok_or("A player is expected.")?;
    let allowed = guard.is_admin(p) || ctx.players.has_permission(p.subject.uuid, "worldguard.region.toggle-bypass");
    if !allowed {
        return Err(NO_PERMISSION.into());
    }
    let want = match a.words.first() {
        None => None,
        Some(w) if w.eq_ignore_ascii_case("on") => Some(true),
        Some(w) if w.eq_ignore_ascii_case("off") => Some(false),
        Some(_) => return Err("Allowed optional arguments are: on, off".into()),
    };
    // `bypasses()` is "bypass unless switched off"; the toggle works on whoever the sender is.
    let on = match want {
        None => !guard.bypasses(p),
        Some(on) => on,
    };
    guard.set_bypass(p, on);
    if on {
        r.print("You are now bypassing region protection (as long as you have permission).");
    } else {
        r.print("You are no longer bypassing region protection.");
    }
    Ok(())
}

const DANGEROUS: &str = "This command is potentially dangerous.\nPlease ensure you have made a backup of your data, and then re-enter the command with -y tacked on at the end to proceed.";

fn migrate_heights(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, r: &mut Reply) -> Res {
    if !plugin_perm(guard, ctx, "region.migrateheights", "migrateheights") {
        return Err(NO_PERMISSION.into());
    }
    if !a.has('y') {
        return Err(DANGEROUS.into());
    }
    let only = check_world(guard, ctx, a, 'w').ok();
    let (lo, hi) = ctx.height;
    if lo == 0 && hi == 255 {
        r.print("Migration complete!");
        return Ok(());
    }
    let names: Vec<String> = guard.worlds.keys().cloned().collect();
    for name in names {
        if only.as_ref().is_some_and(|o| *o != name) {
            continue;
        }
        let dim = dimension_of_world(guard, &name);
        let w = guard.world_mut(&dim);
        let ids: Vec<String> = w.all().map(|x| x.id.clone()).collect();
        for id in ids {
            let reg = w.get_mut(&id).unwrap();
            match &mut reg.shape {
                Shape::Cuboid { min, max } if min.y <= 0 && max.y >= 255 => {
                    *min = BlockPos::new(min.x, lo, min.z);
                    *max = BlockPos::new(max.x, hi, max.z);
                }
                Shape::Polygon { min_y, max_y, .. } if *min_y <= 0 && *max_y >= 255 => {
                    *min_y = lo;
                    *max_y = hi;
                }
                _ => {}
            }
        }
        w.reshaped();
    }
    r.print("Migration complete!");
    Ok(())
}

fn migrate_db(guard: &mut Guard, ctx: &mut Ctx<'_>, a: &Args, _r: &mut Reply) -> Res {
    if !plugin_perm(guard, ctx, "region.migratedb", "migratedb") {
        return Err(NO_PERMISSION.into());
    }
    let (from, to) = (a.words[0].to_ascii_lowercase(), a.words[1].to_ascii_lowercase());
    let known = |s: &str| matches!(s, "mysql" | "sqlite" | "yaml");
    if !known(&from) {
        return Err("The value of 'from' is not a recognized type of region data database.".into());
    }
    if !known(&to) {
        return Err("The value of 'to' is not a recognized type of region region data database.".into());
    }
    if from == to {
        return Err("It is not possible to migrate between the same types of region data databases.".into());
    }
    if !a.has('y') {
        return Err(DANGEROUS.into());
    }
    Err("The driver specified as 'from' does not seem to be supported in your version of WorldGuard.".into())
}

fn migrate_uuid(guard: &mut Guard, ctx: &mut Ctx<'_>, r: &mut Reply) -> Res {
    if !plugin_perm(guard, ctx, "region.migrateuuid", "migrateuuid") {
        return Err(NO_PERMISSION.into());
    }
    r.print("Now performing migration... this may take a while.");
    r.print("Migration complete!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::Config;
    use super::super::selection::Store;

    struct Known;
    impl Players for Known {
        fn has_permission(&self, _uuid: u128, _node: &str) -> bool {
            false
        }
        fn resolve(&self, name: &str) -> Option<(u128, String)> {
            match name.to_ascii_lowercase().as_str() {
                "alice" => Some((1, "Alice".into())),
                "bob" => Some((2, "Bob".into())),
                _ => None,
            }
        }
        fn name_of(&self, uuid: u128) -> Option<String> {
            match uuid {
                1 => Some("Alice".into()),
                2 => Some("Bob".into()),
                _ => None,
            }
        }
    }

    fn player(uuid: u128, op: u8) -> Player {
        Player {
            subject: Subject { uuid, name: format!("p{uuid}"), groups: vec![] },
            op_level: op,
            dimension: "minecraft:overworld".into(),
            pos: [5.0, 64.0, 5.0],
            yaw: 0.0,
            pitch: 0.0,
            game_mode: 0,
        }
    }

    fn run_as(g: &mut Guard, sel: &mut Store, p: Option<&Player>, line: &str) -> Reply {
        let mut ctx = Ctx { sender: p, selections: sel, players: &Known, height: (-64, 319) };
        run(g, &mut ctx, line)
    }

    fn cuboid_sel(uuid: u128, a: [i32; 3], b: [i32; 3]) -> (u128, Selection) {
        (uuid, Selection::Cuboid { dimension: "minecraft:overworld".into(), a: a.into(), b: b.into() })
    }

    fn has(r: &Reply, text: &str) -> bool {
        r.lines.iter().any(|l| l.contains(text))
    }

    #[test]
    fn define_flag_info_remove() {
        let mut g = Guard::new(Config::default());
        let mut sel = Store::default();
        let op = player(9, 4);
        let (u, s) = cuboid_sel(9, [0, 0, 0], [10, 100, 10]);
        sel.0.insert(u, s);
        let r = run_as(&mut g, &mut sel, Some(&op), "define spawn alice g:builders");
        assert_eq!(r.lines[0], "&dA new region has been made named 'spawn'.");
        assert!(has(&r, "NOW PROTECTED"), "{:?}", r.lines);
        let w = g.worlds.get("world").unwrap();
        let reg = w.get("spawn").unwrap();
        assert_eq!(reg.owners.unique_ids, vec!["00000000-0000-0000-0000-000000000001".to_string()]);
        assert_eq!(reg.owners.groups, vec!["builders".to_string()]);
        let r = run_as(&mut g, &mut sel, Some(&op), "flag spawn pvp -g nonmembers deny");
        assert!(has(&r, "Region flag pvp set on 'spawn' to 'DENY'."), "{:?}", r.lines);
        assert!(has(&r, "Region group flag for 'pvp' set."), "{:?}", r.lines);
        run_as(&mut g, &mut sel, Some(&op), "flag spawn greeting Welcome to the spawn");
        let reg = g.worlds["world"].get("spawn").unwrap();
        assert_eq!(reg.flag("pvp"), Some(&FlagValue::State(false)));
        assert_eq!(reg.group("pvp"), Some(Group::NonMembers));
        assert_eq!(reg.flag("greeting"), Some(&FlagValue::Str("Welcome to the spawn".into())));
        let info = run_as(&mut g, &mut sel, Some(&op), "info");
        assert!(has(&info, "Alice"), "{:?}", info.lines);
        assert!(has(&info, "pvp -g NON_MEMBERS: "), "{:?}", info.lines);
        assert!(has(&info, "Region Info"));
        let r = run_as(&mut g, &mut sel, Some(&op), "flag spawn pvp");
        assert!(has(&r, "Region flag pvp removed from 'spawn'. (Any -g(roups) were also removed.)"));
        assert!(g.worlds["world"].get("spawn").unwrap().flag("pvp").is_none());
        assert!(g.worlds["world"].get("spawn").unwrap().group("pvp").is_none());
        let r = run_as(&mut g, &mut sel, Some(&op), "rem spawn");
        assert_eq!(r.lines[0], "&dSuccessfully removed spawn.");
    }

    #[test]
    fn flag_values_and_errors() {
        let mut g = Guard::new(Config::default());
        let mut sel = Store::default();
        let op = player(9, 4);
        let (u, s) = cuboid_sel(9, [0, 0, 0], [10, 100, 10]);
        sel.0.insert(u, s);
        run_as(&mut g, &mut sel, Some(&op), "define plot");
        let r = run_as(&mut g, &mut sel, Some(&op), "flag plot pvp maybe");
        assert_eq!(r.lines[0], "&cExpected none/allow/deny but got 'maybe'");
        let r = run_as(&mut g, &mut sel, Some(&op), "flag plot pvp -e");
        assert!(has(&r, "Expected none/allow/deny"), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "flag plot pvp deny -e");
        assert_eq!(r.lines[0], "&cYou cannot use -e(mpty) with a flag value.");
        let r = run_as(&mut g, &mut sel, Some(&op), "flag plot nosuchflag x");
        assert_eq!(r.lines[0], "&cUnknown flag specified: nosuchflag");
        let r = run_as(&mut g, &mut sel, Some(&op), "flag plot blockbreak deny");
        assert!(has(&r, "WARNING"), "{:?}", r.lines);
        assert_eq!(g.worlds["world"].get("plot").unwrap().flag("block-break"), Some(&FlagValue::State(false)));
        let r = run_as(&mut g, &mut sel, Some(&op), "flag plot greeting -e");
        assert!(has(&r, "Region flag greeting set on 'plot' to ''."), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "flag plot pvp -g members");
        assert!(has(&r, "Region group flag for 'pvp' set."), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "flag plot pvp -g weird deny");
        assert_eq!(
            r.lines[0],
            "&cUnknown value 'weird' in com.sk89q.worldguard.protection.flags.RegionGroup"
        );
        let r = run_as(&mut g, &mut sel, Some(&op), "setpriority plot x");
        assert_eq!(r.lines[0], "&cNumber expected; string \"x\" given.");
        let r = run_as(&mut g, &mut sel, Some(&op), "define plot");
        assert!(has(&r, "To change the shape, use /region redefine plot."), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "info nothing");
        assert_eq!(r.lines[0], "&cNo region could be found with the name of 'nothing'.");
        let r = run_as(&mut g, &mut sel, Some(&op), "setpriority plot");
        assert_eq!(r.lines[0], "&cToo few arguments.");
    }

    #[test]
    fn players_need_permission_and_claims_have_limits() {
        let mut cfg = Config::default();
        cfg.player_commands = vec!["claim".into(), "addmember".into(), "info".into()];
        let mut g = Guard::new(cfg);
        let mut sel = Store::default();
        let me = player(5, 0);
        let r = run_as(&mut g, &mut sel, Some(&me), "define x");
        assert_eq!(r.lines[0], "&cYou don't have permission to do that.");
        let (u, s) = cuboid_sel(5, [0, 0, 0], [100, 100, 100]);
        sel.0.insert(u, s);
        let r = run_as(&mut g, &mut sel, Some(&me), "claim home");
        assert!(r.lines[0].contains("too large"), "{:?}", r.lines);
        let (u, s) = cuboid_sel(5, [0, 60, 0], [9, 69, 9]);
        sel.0.insert(u, s);
        let r = run_as(&mut g, &mut sel, Some(&me), "claim home");
        assert_eq!(r.lines[0], "&dA new region has been claimed named 'home'.");
        let r = run_as(&mut g, &mut sel, Some(&me), "addmember home bob");
        assert_eq!(r.lines[0], "&dRegion 'home' updated with new members.");
        // Someone else may not claim over it.
        let other = player(6, 0);
        let (u, s) = cuboid_sel(6, [5, 60, 5], [12, 69, 12]);
        sel.0.insert(u, s);
        let r = run_as(&mut g, &mut sel, Some(&other), "claim mine");
        assert!(r.lines[0].contains("overlaps with someone else's region"), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&other), "addmember home alice");
        assert_eq!(r.lines[0], "&cYou don't have permission to do that.");
    }

    #[test]
    fn list_leaves_out_the_blank_global_region() {
        let mut g = Guard::new(Config::default());
        let mut sel = Store::default();
        let op = player(9, 4);
        // `/rg flags` and `/rg info __global__` make the global region on demand.
        run_as(&mut g, &mut sel, Some(&op), "flags");
        assert!(g.worlds.values().any(|w| w.global().is_some()), "the global region was made");
        let r = run_as(&mut g, &mut sel, Some(&op), "list");
        assert!(!r.lines.iter().any(|l| l.contains("__global__")), "{:?}", r.lines);
        assert!(has(&r, "(none)"), "{:?}", r.lines);
        // Once something is set on it, it is a region like another.
        run_as(&mut g, &mut sel, Some(&op), "flag __global__ tnt deny");
        let r = run_as(&mut g, &mut sel, Some(&op), "list");
        assert!(r.lines.iter().any(|l| l.contains("__global__")), "{:?}", r.lines);
    }

    #[test]
    fn console_global_parent_teleport() {
        let mut g = Guard::new(Config::default());
        let mut sel = Store::default();
        let r = run_as(&mut g, &mut sel, None, "flag __global__ tnt deny");
        assert!(r.lines[0].contains("-w"), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, None, "flag __global__ tnt -w world deny");
        assert!(has(&r, "Region flag tnt set on '__global__' to 'DENY'."), "{:?}", r.lines);
        assert_eq!(g.worlds["world"].global().unwrap().flag("tnt"), Some(&FlagValue::State(false)));
        let op = player(9, 4);
        run_as(&mut g, &mut sel, Some(&op), "define -g template");
        let (u, s) = cuboid_sel(9, [0, 0, 0], [10, 10, 10]);
        sel.0.insert(u, s);
        run_as(&mut g, &mut sel, Some(&op), "define plot");
        let r = run_as(&mut g, &mut sel, Some(&op), "setparent plot template");
        assert_eq!(r.lines[0], "&dInheritance set for region 'plot'.");
        assert!(has(&r, "template"), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "setparent template plot");
        assert!(has(&r, "would cause circular inheritance"), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "setpriority plot -5");
        assert_eq!(r.lines[0], "&dPriority of 'plot' set to -5 (higher numbers override).");
        let r = run_as(&mut g, &mut sel, Some(&op), "flag template teleport 1,2,3");
        assert!(has(&r, "Region flag teleport set on 'template'"), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "tp plot");
        assert!(matches!(&r.effects[0], Effect::Teleport { pos, .. } if *pos == [1.0, 2.0, 3.0]), "{:?}", r);
        let r = run_as(&mut g, &mut sel, Some(&op), "remove template");
        assert!(has(&r, "has child regions. Use -f to force removal of children or -u"), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "list");
        assert!(has(&r, "plot"), "{:?}", r.lines);
        let r = run_as(&mut g, &mut sel, Some(&op), "select plot");
        assert_eq!(r.lines[0], "&dRegion selected as cuboid");
        assert!(matches!(sel.0.get(&9), Some(Selection::Cuboid { .. })));
    }

    #[test]
    fn owners_members_and_quotes() {
        let mut g = Guard::new(Config::default());
        let mut sel = Store::default();
        let op = player(9, 4);
        let (u, s) = cuboid_sel(9, [0, 0, 0], [10, 10, 10]);
        sel.0.insert(u, s);
        run_as(&mut g, &mut sel, Some(&op), "define plot");
        let r = run_as(&mut g, &mut sel, Some(&op), "addowner -w \"world\" plot alice g:staff Zed");
        assert_eq!(r.lines[0], "&dRegion 'plot' updated with new owners.");
        let reg = g.worlds["world"].get("plot").unwrap();
        assert_eq!(reg.owners.groups, vec!["staff".to_string()]);
        assert_eq!(reg.owners.players, vec!["zed".to_string()]);
        let r = run_as(&mut g, &mut sel, Some(&op), "removeowner plot");
        assert_eq!(r.lines[0], "&cList some names to remove, or use -a to remove all.");
        let r = run_as(&mut g, &mut sel, Some(&op), "removeowner plot -a");
        assert_eq!(r.lines[0], "&dRegion 'plot' updated with owners removed.");
        assert!(g.worlds["world"].get("plot").unwrap().owners.is_empty());
        let r = run_as(&mut g, &mut sel, Some(&op), "bypass maybe");
        assert_eq!(r.lines[0], "&cAllowed optional arguments are: on, off");
        let r = run_as(&mut g, &mut sel, Some(&op), "bypass off");
        assert_eq!(r.lines[0], "&dYou are no longer bypassing region protection.");
        let r = run_as(&mut g, &mut sel, Some(&op), "toggle-bypass");
        assert_eq!(r.lines[0], "&dYou are now bypassing region protection (as long as you have permission).");
        assert_eq!(tokenize("a \"b c\" d"), vec!["a".to_string(), "b c".to_string(), "d".to_string()]);
    }
}
