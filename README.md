# horizon-worldguard

> **Unofficial Rust port of WorldGuard for the Horizon server (https://github.com/arismoko/horizon). Not affiliated with or endorsed by the WorldGuard authors.**

Region protection for [Horizon](https://github.com/arismoko/horizon) dedicated servers, as a
native trait mod: cuboid and polygon regions with priorities, parents, owners and members, the
region flags, the `/rg` commands, and the same `regions.yml` files as the Paper plugin, so a server
can move between the two without converting anything.

Not affiliated with WorldGuard, sk89q or EngineHub.

**Derived from WorldGuard (https://github.com/EngineHub/WorldGuard, LGPL-3.0-or-later).** The rules,
flags, defaults, commands and messages follow WorldGuard's source (the `RegionCommands`,
`MemberCommands`, `FlagValueCalculator`, `RegionProtectionListener`, session handlers and
`Flags` classes), ported to Rust for Horizon. The licence of this mod is therefore the
GNU Lesser General Public License version 3 (see LICENSE); the copyright holders are in NOTICE.

> **Built for and working only with [arismoko/horizon](https://github.com/arismoko/horizon)** (the Horizon fork that carries the mod API). [horizon-rust/horizon](https://github.com/horizon-rust/horizon) is experimental for mods and not supported: the earlier version for its native hooks / horizon-mixins is frozen on the `horizon-plain` branch (work stopped on 2026-11-12).

## Which Horizon

This is a native trait mod: `horizon-mod.toml`, a `Cargo.toml` that depends on `horizon-mod-api`, `src/lib.rs` with the hooks, and the plugin logic in the `core/` crate (plain Rust, no engine needed for its tests).

- Pin: `[native] horizon = "54090ee36c4e2c7b43b66db864220c577b94b7e0"` in `horizon-mod.toml` (game `26.3`). That is a development build of arismoko/horizon `main` plus mod API additions that are not all merged upstream yet, so a stock `main` may lack some host methods this mod calls until they land.
- It compiled and loaded on that build and was smoke-tested there; it has not seen a long run with real players. Treat it as experimental.
- Feature descriptions below were written for the earlier horizon-rust version; where they name engine calls (`native_*`) or old file paths, read them as descriptions only. The Known gaps list is current.

## What it does

- Regions: cuboid, 2D polygon (with a height range), the per-world `__global__` region, and
  template regions without area (`define -g`), each with a priority, an optional parent, owners and
  members (UUIDs, names, `g:` groups).
- Flag resolution as documented: highest priority first, deny beats allow at one priority, children
  inherit flags and members from parents and override lower or equal ones, region groups
  (`-g nonmembers` ...), and the build rule (a specific flag beats `build`, otherwise membership of
  every top-priority protecting region; `passthrough`; the global region only protects with
  `passthrough deny` or members).
- Protection through Horizon's hooks: breaking, placing, using blocks and containers, buckets,
  frames and paintings, attacking animals, vehicles and players (melee and projectiles), riding,
  mob spawning, explosions, fire, leaf decay, fluids, pistons, ender pearls, chorus fruit, item
  pickup, chat, commands, sleeping, invincibility and fall damage; entry/exit, greeting/farewell,
  notifications and game mode while walking.
- Commands: `/rg define, redefine, claim, select, info, list, flags, flag, setpriority, setparent,
  remove, addmember, addowner, removemember, removeowner, teleport, load, save, bypass,
  migrateheights`, with the documented aliases and options; `/wg reload|flushstates|running|version`.

Full list with what is partial: the Known gaps section. What Horizon lacks for the rest: the Known gaps section.

## Selections

Selections come from the world editing mod (bundle id `worldedit_server`, horizon-worldedit's server bundle), an optional dependency. The two
functions it has to publish are in SELECTION.md.

## Files

- Regions: `<server>/<data-folder>/worlds/<world>/regions.yml`, default
  `plugins/WorldGuard/worlds/world/regions.yml` (and `world_nether`, `world_the_end`). Written a few
  seconds after a change and when the server stops.
- Config: `config/worldguard-common.toml`, created with defaults on first start and re-read when it
  changes. Keys follow WorldGuard's `config.yml`:

```toml
op-permission-level = 2          # operators of this level have every permission (op-permissions: true)
data-folder = "plugins/WorldGuard"
save-delay-seconds = 5
player-commands = []             # /rg subcommands non-operators may run (on regions they own)
use-player-move-event = true
use-player-teleports = true

[worlds]                         # world folder name per dimension
"minecraft:overworld" = "world"
"minecraft:the_nether" = "world_nether"
"minecraft:the_end" = "world_the_end"

[regions]                        # WorldGuard's `regions:` section, with its defaults
enable = true
disable-bypass-by-default = false
announce-bypass-status = false
use-creature-spawn-event = true
invincibility-removes-mobs = false
cancel-chat-without-recipients = true
nether-portal-protection = true
titles-always-use-default-times = true
fake-player-build-override = true
explosion-flags-block-entity-damage = true
high-frequency-flags = false
protect-against-liquid-flow = false
wand = "minecraft:leather"
max-claim-volume = 30000
claim-only-inside-existing-regions = false
set-parent-on-claim = ""
location-flags-only-inside-regions = false
piston-border-protection = true  # this mod's own key

[regions.max-region-count-per-player]
default = 7                      # plus a number per permission group

[protection]
use-max-priority-association = false

[messages]                       # this mod's overrides of the deny-message flags' defaults
deny = "&c&lHey!&7 Sorry, but you can't %what% here."
entry-deny = "&c&lHey!&7 You are not permitted to enter this area."
exit-deny = "&c&lHey!&7 You are not permitted to leave this area."

[mobs]                           # WorldGuard's per-world sections, merged into this file:
block-creeper-explosions = false # the mobs, ignition, fire, dynamics and player-damage keys that
                                 # have a handler (see config.rs), all false by default
```

Permissions follow WorldGuard's `RegionPermissionModel`: operators (`op-permission-level`) have every
right; other players need the node (`worldguard.region.define`, `.claim`, `.list`, `.list.own`,
`.load`, `.save`, `.migrate*`, `.unlimited`, `.toggle-bypass`, and for one region
`worldguard.region.<command>.<id>`, `.member.<id>` or `.own.<id>`, with `flag.regions` and
`flag.flags.<flag>[.<value>]`) through Horizon's `has_permission` (a permissions mod answers it; without
one only operators pass), or a subcommand listed in `player-commands`. Bypass nodes are not read yet
(the Known gaps section).

## Build

Build it as a native trait mod with arismoko's launcher (see the "native mods" how-to in the arismoko/horizon docs):

```sh
horizon-launcher native-build <path to this folder> --server-output <dir> --version <tag> --trust-native-code
```

`--trust-native-code` is your consent to compile and run this code inside the server. The logic crate tests run without the engine: `(cd core && cargo test)`.

## Known gaps

What does not work yet on arismoko/horizon (each is marked `TODO(arismoko-api: ...)` in `src/lib.rs`):

- `/rg define`, `redefine`, `claim` and `select` report that there is no selection (no WorldEdit selection API on arismoko yet).
- Colour codes are dropped (messages are plain), and titles go to the action bar.
- Portal use, chorus-fruit and `/tp` teleports cannot be refused; a rescue teleport puts the player back on the next tick.
- The source entity of an explosion and the kind of a spreading fluid are not told: the nearest tracked explosive names the blast, water-flow and lava-flow apply to any fluid.
- Farmland trampling and a vehicle leaving a region cannot be held back.
