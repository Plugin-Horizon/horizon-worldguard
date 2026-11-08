# Status (2026-11-06, written blind: no engine build yet)

Legend: **written** = code and rule done (core rules unit-tested), glue written against the
a589a3dd9 hook signatures but not compiled with the engine; **partly** = works with a workaround or
for part of the cases; **not started** = blocked or left out (GAPS.md says why).

The engine-free core (`api/src/wg/`, tested through `core/`) has 38 unit tests, all green. The glue
(`api/src/lib.rs`) was type-checked only against a hand-written stand-in of `mod_api`, so the real
event field types are still a guess where marked `UNVERIFIED`.

## Data

| Feature | Status |
|---|---|
| `regions.yml` read: cuboid, poly2d, global, priority, parent, owners/members (players, unique-ids, groups), flags, `<flag>-group`, unknown flags and keys kept | written |
| `regions.yml` write (same layout: 4-space blocks, flow maps for scalar-only maps, header comment), atomic rename | written |
| Same files as the Paper plugin (`plugins/WorldGuard/worlds/<world>/regions.yml`) | written |
| Unreadable file never overwritten; broken/circular parents dropped with a log line | written |
| MySQL region storage | not started (not used in the reference setup; docs advise against it) |
| Config (`config/worldguard-common.toml`, WorldGuard key names, hot reload) | written |

## Regions and queries

| Feature | Status |
|---|---|
| Cuboid / polygon / global / template (`define -g`) regions | written |
| Priorities, parent inheritance (flags and members), child sets aside lower/equal parents | written |
| Region groups (`-g all/members/owners/nonmembers/nonowners/none`), flag default groups (`entry`/`exit` non-members, `spawn` members) | written |
| Build query: specific flag > `build` > membership of all top-priority protecting regions, `passthrough`, global region rules | written |
| Point query with a chunk index | written |

## Commands (`/rg`, `/region`, `/regions`; `/wg reload|flushstates|running|version`)

Command tree (`api/src/lib.rs` `register_commands`, `api/src/wg/commands.rs` `usages`): one usage per
subcommand path, typed (`<id:word>`, `<page:integer>`, `<player:player_name>`, `<names:text>`), fixed
choices as literals (`on|off`, `-w`, `-g`, `-e`, flag names from `flags::FLAGS`). No catch-all. Not in the
tree: subcommand aliases (`create`, `tp`, ...) and the `-g` group names (a word, not six literals).

Checked against the public command pages (2026-11-08): all 20 documented `/rg` subcommands and their
aliases are present, with their options (`-w`, `-g`, `-n`, `-f`, `-u`, `-s`, `-i`, `-p`, `-a`, `-e`,
`-c`). Documented `/rg` options the port does not take: none. Documented `/wg` subcommands: below.

| Command | Status |
|---|---|
| define / redefine (with owners, `-g`, `-n`) | written (needs the world editing mod's selection: `worldedit_server::selection` from horizon-worldedit's server bundle, which has an `[api]` package since 2026-11-08; the dependency was named `worldedit`, the id of the window bundle, which has none) |
| claim (volume, count, overlap, inside-own-region, parent-on-claim) | written |
| select, info (`-s`, `-u`), list (`-i`, `-p`, `-s`, pages), flags (pages, inherited marked) | written |
| flag (`-g`, `-e`, unset, `here` for locations, global region on demand) | written |
| setpriority, setparent (cycle check), remove (`-f`, `-u`) | written |
| addmember/addowner/removemember/removeowner (`g:group`, `-a`, `-n`) | written (offline names only by name: GAPS.md) |
| teleport (`-s`, `-c`), teleport-message | partly (queued `tp` command) |
| load / save, migrateheights | written |
| bypass toggle, `/rg bypass on|off`, announce-bypass-status | written (operators only: non-operators get a message) |
| `nether-portal-protection`: build check at the destination on `player_portal` (cause `nether_portal`) | written (approximate: x/z scaled by 8, same y) |
| `fake-player-build-override` (names `[...]` bypass in `Guard::bypasses`), `location-flags-only-inside-regions` (location flags must be inside the region) | written |
| `/wg flushstates [player]` (alias `clearstates`): clears the movement state of all players or one seen player | written |
| `/wg running` (alias `queue`), `/wg reload`, `/wg version` | written (`running` is always empty: no background tasks) |
| `/wg report`, `/wg profile`, `/wg stopprofile`, `/wg debug ...` | not started (answer "not available"; GAPS.md) |
| migratedb / migrateuuid | not started (YAML only; UUIDs resolved on add) |
| Permission nodes: `worldguard.region.<command>` and its `.*` form (through `host.has_permission`, operators by level), `worldguard.region.unlimited` (claim limits) | partly (no `.own.`/`.member.`/per-flag forms, no bypass nodes: GAPS.md) |
| Region wand (`wand` item, right click lists regions) | written |

## Flags enforced

| Flags | Status |
|---|---|
| build, block-break, block-place | written (block-place through the cancellable `block_place` hook) |
| interact, use, chest-access, sleep, respawn-anchors, use-anvil, use-dripleaf, lighter | written |
| damage-animals, pvp (melee and projectiles), item-frame-rotation, ride, vehicle-place, vehicle-destroy | written |
| tnt, creeper-explosion, ghast-fireball, wither-damage, enderdragon-block-damage, other-explosion | partly (source guessed: GAPS.md) |
| mob-spawning, deny-spawn | written |
| invincible, fall-damage, mob-damage, firework-damage | written (mob-damage partly) |
| fire-spread, lava-fire, lightning, leaf-decay, water-flow, lava-flow, pistons, piston borders | written (water/lava-flow checked together) |
| entry, exit, exit-override, exit-via-teleport, entry/exit-deny-message | written (moves refused by the `player_move` hook; teleports and rides pushed back at the next tick) |
| greeting, farewell, notify-enter, notify-leave | written |
| greeting-title, farewell-title | written (`Host::title`) |
| game-mode | partly (console command) |
| enderpearl, chorus-fruit-teleport, item-pickup, send-chat, blocked-cmds, allowed-cmds, deny-message, teleport | written |
| entity-painting-destroy, entity-item-frame-destroy | partly (overworld only without a player) |
| item-drop | written (`item_dropped_v2`) |
| receive-chat, potion-splash, spawn, time-lock, weather-lock, heal-*, feed-*, natural-health-regen, natural-hunger-drain, exp-drops | not started (hooks/helpers missing) |
| enderman-grief, ravager-grief, snowman-trails, block-trampling, frosted-ice-form, ice/snow melt and form, soil-dry, coral-fade, copper-fade, growth flags | not started (rules written in `Guard`, hooks missing) |
| nonplayer-protection-domains | not started (stored and written back only) |

## World config (`worlds/<world>/config.yml`, main config world part)

All of it is at defaults in the reference setup (every `disable-*`/`block-*` switch off, blacklist empty), so
none of it is ported. `high-frequency-flags`, `protect-against-liquid-flow` and
`explosion-flags-block-entity-damage` are read from the TOML.

## Test first, once a build is possible

1. Compile `api/` against the engine; fix the `UNVERIFIED` spots (level hooks' `dimension` and
   `BlockPos`, `EnderPearlTeleportEvent::target`, `CanPlayerSleepEvent::problem`, event lifetimes).
2. Load a copy of a real `regions.yml` (not production data: build one with the Paper plugin on a
   throwaway server), `/rg save`, and diff: the file must read back unchanged in the Paper plugin.
3. Block break/place/use in a region as a non-member, a member and an operator (with and without
   `/rg bypass`); the placement undo (block back, item refunded once).
4. PvP melee and bow in a `pvp deny` region; `mob-spawning deny`; `tnt deny` on `__global__`.
5. Entry deny push-back and greeting/farewell walking in and out.
6. Selection commands together with the world editing mod's `[api]`.

## Round 3 (2026-11-08): ported against WorldGuard's source (LGPL-3.0-or-later)

Checked line by line against `worldguard-core` (flags, `FlagValueCalculator`, `RegionCommands`,
`MemberCommands`, `RegionPermissionModel`, session handlers) and `worldguard-bukkit`
(`RegionProtectionListener`, `RegionFlagsListener`, `Materials`, `Entities`, `defaults/`). Done:

- Flag table in registration order, with the missing flags (`wind-charge-burst`,
  `breeze-charge-explosion`, `moisture-change`, `lava-harden`), the real defaults (`pvp`, `potion-splash`
  and `frosted-ice-form` are protection flags, `exit-via-teleport` allows) and fuzzy name matching
  (`blockbreak`). Flag values are parsed as `Flag.parseInput` does (`none/allow/deny`, yes/no, `x,y,z`,
  command sets, registry names) with its error texts.
- `query.rs` is `FlagValueCalculator`: parents join the walk (with their priority), `build` is implicit
  on every region without `passthrough allow`, membership as the default, the global region's special
  cases, `testBuild` as `build` combined with the specific flags.
- `/rg`: every subcommand and alias, flag letters, argument counts with the usage line, quoting,
  permission nodes per region (`.own.`/`.member.`), the exact replies, errors and warnings, `flag -h`,
  `flag -g` without a value, `flags` help pages, `list` pages/owner marks, `setparent` trees, `remove`
  child rule, `migrate*` confirmation, `/wg` replies.
- Protection: block use/place/break, items and buckets, entities, PvP (bypass does not apply), vehicles,
  frames, explosions (flag + source regions + the "one blocked block cancels the blast" rule), the
  `Materials` click table (the 758-entry flag table is in `kinds.rs`), deny message macros, 500 ms delay.
- Movement: entry is checked at the destination, exit by the change of its state, 2 s message delay,
  greeting/farewell stacks, `notify-*` texts, default deny messages with `Hey!`.
- Config: `regions.*` keys with WorldGuard's defaults, `max-region-count-per-player.default` and groups,
  the explosion, fire, dynamics and player-damage switches.

Checked with `rustc` on `api/src/wg/` alone (the engine-free core, 45 tests): `lib.rs` was not compiled.

## Live test run, 2026-11-08

- `/rg define`, `claim`, `redefine` said `Please select an area first` because no installed mod published the
  selection: the optional dependency named `worldedit` (the upstream window bundle, no `[api]`). It is now
  `worldedit_server` (feature of the same name), the id of the server bundle, which publishes
  `selection::{selection_of, set_selection}` over its engine. Not compiled (no engine build).
- `/rg list` listed `__global__` on a world with no region: `/rg flags` and `/rg info __global__` make an empty global
  region on demand (as WorldGuard's `checkExistingRegion` does), and it was listed like a defined one. It is left out
  until something is set on it; `/wg reload` counts regions the same way. Test `list_leaves_out_the_blank_global_region`.
