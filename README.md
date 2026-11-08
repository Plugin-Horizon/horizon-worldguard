# horizon-worldguard

> **Unofficial Rust port of WorldGuard for the Horizon server (https://github.com/horizon-rust/horizon). Not affiliated with or endorsed by the WorldGuard authors.**

Region protection for [Horizon](https://github.com/horizon-rust/horizon) dedicated servers, as a
native trait mod: cuboid and polygon regions with priorities, parents, owners and members, the
region flags, the `/rg` commands, and the same `regions.yml` files as the Paper plugin, so a server
can move between the two without converting anything.

Not affiliated with WorldGuard, sk89q or EngineHub.

**Derived from WorldGuard (https://github.com/EngineHub/WorldGuard, LGPL-3.0-or-later).** The rules,
flags, defaults, commands and messages follow WorldGuard's source (the `RegionCommands`,
`MemberCommands`, `FlagValueCalculator`, `RegionProtectionListener`, session handlers and
`Flags` classes), ported to Rust for Horizon. The licence of this mod is therefore the
GNU Lesser General Public License version 3 (see LICENSE); the copyright holders are in NOTICE.

**State: written without building the engine (see STATUS.md and GAPS.md).** The rules are
unit-tested (`core/`); the hook glue has not run yet.

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

Full list with what is partial: STATUS.md. What Horizon lacks for the rest: GAPS.md.

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
(GAPS.md).

## Layout and build

- `api/` — the trait mod (`[api]` in `horizon-mixin.toml`): `src/lib.rs` maps Horizon's hooks to
  the rules, `src/we_selection.rs` adapts the world editing mod's selections.
- `api/src/wg/` — the rules, with no Horizon types: YAML, shapes, flags, regions, region files,
  queries, protection decisions, movement, commands.
- `core/` — builds `api/src/wg/` as a crate of its own: `cd core && cargo test`.

Build the mod as any trait mod: put this folder into the launcher's mods and build the game
(docs/native-mods.md, "Trait mods"). It needs no injections.

## Untested

Everything in `api/src/lib.rs` (never compiled against the engine), and the round trip with files
written by the Paper plugin beyond the documented layout. STATUS.md lists what to test first.

## Contributing

Pull requests and improvements are always welcome! Open an issue or a PR -- direct pushes to main are disabled, every change goes through review.
