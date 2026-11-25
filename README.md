# horizon-worldguard

## Port status

Compared with WorldGuard 7.0.22-SNAPSHOT (59e136a), by the item list in `audit/rec100/worldguard.md`: **84 %** of the original's commands, permissions, config keys, messages and features work today (36 of 43 items); **95 %** once our open Horizon PRs are merged; **95 %** with the engine changes we are writing; **84 %** with the newest Horizon SDK (the two engine branches below are not in it). Round 6 (2026-11-25): written against the combined next SDK, first build pending; the `wave2` cargo feature (off by default) holds the code for two engine branches the SDK does not have yet.

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
  migrateheights`, with the documented aliases and options; `/wg reload|flushstates|running|report|version`;
  `/god`, `/ungod`, `/heal`, `/slay` (with `-s`, `*` and the `.other` nodes), `/stopfire`, `/allowfire`,
  `/halt-activity` (`/stoplag`) and `/stack`.
- Flags with their own handler: `block-trampling`, `crop-growth`, `moisture-change`, `lava-harden`,
  `exp-drops`, `enderman-grief`, `snowman-trails`, `ravager-grief`, `wind-charge-burst`,
  `natural-health-regen`, `natural-hunger-drain`, `heal-*`, `feed-*`, and `potion-splash` for lingering
  clouds. `weather-lock` and `time-lock` send the player their own weather and time while they stand in the region.

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
auto-invincible = false          # worldguard.auto-invincible starts god mode (older name: auto-invincible-permission)
auto-invincible-group = false    # permission group wg-invincible starts god mode
auto-no-drowning-group = false   # permission group wg-amphibious does not drown
use-particle-effects = true      # smoke where a break or place was refused
uuid-migration-on-start = true   # names in owner/member lists move to unique ids at the first load

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

[mobs]                           # WorldGuard's per-world sections, merged into this file (a world's own
                                 # worlds/<world>/config.yml overrides them for that world):
block-creeper-explosions = false # the mobs, ignition, fire, dynamics and player-damage keys that
                                 # have a handler (see config.rs), all false by default
```

Permissions follow WorldGuard's `RegionPermissionModel`: operators (`op-permission-level`) have every
right; other players need the node (`worldguard.region.define`, `.claim`, `.list`, `.list.own`,
`.load`, `.save`, `.migrate*`, `.unlimited`, `.toggle-bypass`, and for one region
`worldguard.region.<command>.<id>`, `.member.<id>` or `.own.<id>`, with `flag.regions` and
`flag.flags.<flag>[.<value>]`) through Horizon's `has_permission` (a permissions mod answers it; without
one only operators pass), or a subcommand listed in `player-commands`. `worldguard.region.bypass.<world>`
is read once a second for every online player (default: operators), so a permission plugin can grant it.

## Build

Build it as a native trait mod with arismoko's launcher (see the "native mods" how-to in the arismoko/horizon docs):

```sh
horizon-launcher native-build <path to this folder> --server-output <dir> --version <tag> --trust-native-code
```

`--trust-native-code` is your consent to compile and run this code inside the server. The logic crate tests run without the engine: `(cd core && cargo test)`.

## Known gaps

What does not work yet on arismoko/horizon (each is marked `TODO(arismoko-api: ...)` or `HMOD-GAP(...)` in `src/`):

- Colour codes are dropped (messages are plain), and titles go to the action bar.
- Round 2 on the next SDK (written blind, not built yet): `weather-lock` and `time-lock` (per-player `game_event` and `set_time` packets), `vine-growth` and `sculk-growth` (`natural_block_spread`, `block_grow`), `potion-splash` splash hits (`potion_splash_cancel`, read at the landing spot), `mobs.block-zombie-door-destruction` (`entity_break_door_cancel`), `player-damage.disable-death-messages` (`player_death_message`), blacklist `on-dispense` (`block_dispense`) and `ignore-groups`, `/halt-activity` removing entities, `/rg migrateuuid` (server profile lookup), region-scoped permission nodes (`permission_declare_node`, no feature gate any more).
- Colour codes are dropped (messages are plain), and titles go to the action bar.
- A rescue teleport (void, suffocation) puts the player back on the next tick.
- The source entity of an explosion is not told: the nearest tracked explosive names the blast.
- Round 3 (written blind, first build pending): `physics.no-physics-sand|gravel|allow-portal-anywhere|vine-like-rope-ladders` (`block_physics_cancel`, sources `falling` and `neighbor`), `grass-growth`, `mycelium-spread`, `rock-growth` (`block_growth_cancel`, kinds `grass`, `mycelium`, `dripstone`), `mobs.block-enderdragon-portal-creation` (`portal_create_entity_cancel`), `natural-health-regen` by cause (`living_heal_cause_cancel`), blacklist `on-equip` (`armor_equip_cancel`), per-player weather and time, `/locate` with and without a player (`player_set_compass_target`, `level_respawn_data`). The `next-engine` feature is gone.
- Round 6 (written blind, first build pending): `/wg debug testbreak|testplace|testinteract|testdamage <player> [-t] [-s]` (a ray trace from the sender's eye, then the same guard checks the hooks use; the report lists WorldGuard's answer only), `worldguard.auto-invincible`, `auto-invincible-group`, `auto-no-drowning-group` (amphibious: no drowning), the `invincible` flag as a session handler (a deny beats god mode unless `worldguard.god.override-regions`; fire put out, no hunger, `invincibility-removes-mobs`), `default.pumpkin-scuba` (helmet read twice a second), the `player-portal` check (`entity_travel_to_dimension`, cause `nether_portal`), `/tp` and `/teleport` refusal for `entry` and `exit` (`teleport_command`, `exit-via-teleport` (allowed by default) lets a teleport out), a rider held on a tameable mount where `ride` is denied (`entity_mount` dismount), smoke where a break or place was refused (`use-particle-effects`), and `uuid-migration-on-start`.
- Cargo feature `wave2` (off by default; the hooks of the engine branches `fam-worldguard-gameplay-cancels` and `fam-diagnostics-profiler`, written from their drafts, not compiled against them): `gameplay.disable-conduit-effects` and `gameplay.block-potions` (`mob_effect_apply_cancel`; the hook names the target, not the thrower, so `worldguard.override.potions` is read for the target), `protection.remove-infinite-stacks` (`player_oversize_stack`: a stack above its item's maximum, since the game has no negative counts), `mobs.anti-wolf-dumbness` (`living_incoming_damage_detail`), `/wg profile [-p] [-i n] [-t filter] [-c] [minutes]` and `/wg stopprofile` (`profiler_start|report|stop`: the engine times mod hooks, tick steps, mob and block entity types instead of sampling threads; the report goes to `profile.txt`), and `/wg debug trace start|take|stop` (`event_trace_*`). Build with `--features wave2` once the SDK has them.
- Still engine (audit `ENGINE_NEEDS.md`): `protection.item-durability` (`damage_item` only reaches items that carry a part), the paste upload of `/wg report -p` and `/wg debug ... -s` (no HTTP host call), TNT lit by an arrow cannot be refused (`tnt_primed_detail` names the shooter but only listens).
- `potion-splash` is read at the landing spot of the potion, not at each affected entity. `time-lock` needs the world clock's registry id (`minecraft:world_clock` / `minecraft:overworld`); if the host does not know it, the lock is silently not shown.
- The void and suffocation teleports use the player's position from a few seconds earlier. `lava-harden` acts on any fluid that places a block. `enderman-grief`, `snowman-trails` and `ravager-grief` look at the mob's own position. A redstone-powered sponge's toggling is not simulated.
- Blacklist: no database log; the file log writes WorldGuard's default path or `logging.file.path`, in UTC. SQL region storage is not ported.
