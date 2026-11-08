# What Horizon is missing for this mod

Checked against Horizon a589a3dd9 (hook API 411). Each line: what the mod would need, which feature
it serves, and what the mod does today.

## Hooks

- [x] A cancellable placement hook before the block is set — needed for `block-place` / `build` on
  block placement — done on the `hooks-all` branch: `Hooks::block_place` (was: undone after the fact
  with a queued `/setblock` and one item handed back).
- [ ] `native_explosion_detonate(dimension, center, source: Option<(u32, &str)>, blocks)` (the
  exploding entity's id and type) — needed for `tnt`, `creeper-explosion`, `ghast-fireball`,
  `wither-damage`, `enderdragon-block-damage`, `other-explosion` — today: the nearest TNT, creeper,
  fireball, wither skull, wither, dragon or end crystal (tracked from `entity_join_level`) within 3
  blocks of the centre is taken as the source; anything else is `other-explosion`.
- [ ] `dimension: &str` in `native_tnt_primed` and `native_hanging_break` — needed for `tnt` and
  `entity-painting-destroy` / `entity-item-frame-destroy` without a player — today: the overworld's
  regions decide when no player is behind it.
- [x] `native_item_dropped(player, item, count, canceled: &mut bool)` (cancellable) — needed for
  `item-drop` — done on the `hooks-all` branch: `Hooks::item_dropped_v2`.
- [ ] `native_entity_change_block(entity, kind, pos, from, to, canceled: &mut bool)` before the
  change (today listen-only, after it) — needed for `enderman-grief`, `ravager-grief`,
  `snowman-trails`, mob trampling — today: not possible.
- [ ] `native_block_trample(entity: u32, kind: &str, pos, canceled: &mut bool)` (farmland, turtle and
  sniffer eggs) — needed for `block-trampling` — today: not possible.
- [ ] `native_block_grow(dimension, pos, new_state, canceled: &mut bool)` for natural spreading and
  growth (mushrooms, grass, mycelium, vines/kelp, dripstone, sculk, crops) — needed for
  `mushroom-growth`, `grass-growth`, `mycelium-spread`, `vine-growth`, `rock-growth`,
  `sculk-growth`, `crop-growth` — today: not possible (the rule is written: `Guard::block_grow`).
- [ ] `native_potion_splash(item, pos, canceled: &mut bool)` (today listen-only) — needed for
  `potion-splash` — today: not possible.
- [ ] `native_player_respawn_position(player, dimension: &mut String, pos: &mut [f64; 3])` before
  the respawn — needed for the `spawn` location flag — today: not possible.
- [ ] A per-recipient chat hook, e.g. `native_chat_recipients(sender: u128, recipients: &mut
  Vec<u128>)` — needed for `receive-chat` — today: not possible (`send-chat` works through
  `native_player_chat`).
- [ ] `native_exp_drop(dimension, pos, amount: &mut i32)` for every experience source (blocks, mobs,
  smelting) — needed for `exp-drops` — today: not started (`native_block_exp` covers blocks only).
- [ ] Natural regeneration cancel, e.g. `native_player_regen(player, amount: &mut f32, canceled)` —
  needed for `natural-health-regen` — today: not started (`natural-hunger-drain` could use
  `native_food_level_changed`, not wired yet).
- [ ] The causing entity's type in `native_living_incoming_damage` (`causing_kind: &str`) — needed
  for `mob-damage` — today: any non-player causing entity counts as a mob.

## Helpers

- [ ] `LevelHost::state_name(state: BlockStateId) -> Option<&str>` (`GameLevel::native_state_block`
  exists, the level host does not expose it), or the fluid's id in `native_block_spread` — needed for
  `water-flow` vs `lava-flow`, and for `ice-melt`, `snow-melt`, `frosted-ice-melt`, `soil-dry`,
  `coral-fade`, `copper-fade` (`native_block_fade`) and `ice-form`, `snow-fall`, `frosted-ice-form`
  (`native_block_form`), which only give state ids — today: a fluid flow is checked against both
  `water-flow` and `lava-flow`; the fade/form flags are not enforced (rules written:
  `Guard::block_fade`, `Guard::block_form`).
- [x] `native_teleport_player(uuid, dimension: &str, pos: [f64; 3], yaw: f32, pitch: f32) -> bool`,
  usable inside hooks — needed for `entry`/`exit` push-back and `/rg teleport` — done on the
  `hooks-all` branch: `Host::teleport_player` (was a `tp` console command queued for the next tick).
- [ ] `native_set_game_mode(uuid, mode: i32) -> bool` — needed for the `game-mode` flag — today: a
  queued `gamemode` console command.
- [x] `native_title(uuid, title: Text, subtitle: Text, fade_in: i32, stay: i32, fade_out: i32) ->
  bool` — needed for `greeting-title` / `farewell-title` — done on the `hooks-b-all` branch:
  `Host::title` (was the action bar).
- [ ] `native_player_time(uuid, ticks: Option<i64>, relative: bool)` and
  `native_player_weather(uuid, raining: Option<bool>)` — needed for `time-lock` and `weather-lock` —
  today: not possible.
- [ ] `native_set_player_health(uuid, health: f32)` and `native_set_player_food(uuid, food: i32)` —
  needed for `heal-*` and `feed-*` — today: not started.
- [ ] Permissions: `native_has_permission(uuid, node: &str) -> Option<bool>` and
  `native_player_groups(uuid) -> Vec<String>` (a permissions mod would answer) — needed for the
  `worldguard.*` nodes (claiming, per-subcommand rights, `.own.`/`.member.` variants, bypass per
  world), `g:<group>` owners/members, and `nonplayer-protection-domains` style group logic — today:
  operator level (`op-permission-level`) for everything, and the config's `player-commands` list for
  non-operators.
- [ ] `native_profile_by_name(name: &str) -> Option<(u128, String)>` (the user cache) — needed for
  `/rg addmember <offline player>` to store a UUID — today: only names seen this session resolve,
  others are stored as names (as WorldGuard's `-n`).
- [ ] An output for console commands (`native_perform_console_command` has no reply channel) — needed
  for `/rg` from the console — today: replies go to the log (`eprintln!`).
- [ ] `native_world_height` for a named dimension — needed for `/rg migrateheights -w <world>` —
  today: the height of the level in place.

## Crates

- [ ] A YAML crate in Horizon's `Cargo.lock` (for example `serde_yaml` or `saphyr`) — needed for
  `regions.yml` — today: `api/src/wg/yaml.rs`, a hand-written reader/writer for the subset region
  files use.

## Other mods

- [ ] The world editing mod's `[api]` package with `selection::selection_of` / `set_selection`
  (SELECTION.md) — needed for every selection command — today: without it, define/claim/redefine
  report "no selection".

## Public-docs parity check (2026-11-08)

Compared with the public WorldGuard command, flag and config pages. All 99 documented flag names
are in the port (`api/src/wg/flags.rs`); the gaps below are commands and config keys.

- `/wg report`, `/wg profile`, `/wg stopprofile`, `/wg debug testbreak|testplace|testinteract|testdamage`
  answer "not available yet". Needs: a report writer (server and config dump), a CPU
  sampler, and a way to fire a simulated block or damage event through Horizon's hooks.
- Config keys the docs list and the port does not read (documented default in brackets):
  `op-permissions` [true] (the port uses `op-permission-level`), `summary-on-start` [true],
  `auto-invincible` [false], `auto-invincible-group` [false], `auto-no-drowning-group` [false],
  `use-player-teleports` [true], `use-particle-effects` [true], `disable-permission-cache` [false],
  `custom-metrics-charts` [true], `host-keys` [""],
  `invincibility-removes-mobs` [false], `cancel-chat-without-recipients` [false],
  `titles-always-use-default-times` [true]. `regions.uuid-migration.*` and `regions.sql.*` do not apply
  (YAML only). Paper-only, not applicable: `use-paper-entity-origin`, `use-creature-spawn-event`.
  Read since then: `fake-player-build-override`, `nether-portal-protection`,
  `location-flags-only-inside-regions` (see the permission section below).
- Keys the port reads that the docs do not list: `op-permission-level`, `player-commands`,
  `save-delay-seconds`, `piston-border-protection` (the `pistons` flag is documented; a separate
  border switch is not). They stay as the port's own keys.
- `/wg save` was removed: it is not a WorldGuard command (`/rg save` is).
- Guesses (wording not in the docs, check against the real plugin when it can be tested):
  `api/src/lib.rs` `wg_command`: the replies for `flushstates`, `running`, the unknown-subcommand
  line and the "not available" line; `api/src/wg/guard.rs` `set_bypass` and the `/rg bypass on|off`
  reply in `api/src/wg/commands.rs` `run` ("bypass" arm).

## Permission nodes and the config keys that now work (2026-11-08)

Source for the node list: the public permissions page (worldguard.enginehub.org/en/latest/permissions/).
The real jar's `plugin.yml` (7.0.20, data file only) declares no permissions.

- Used: `worldguard.region.<command>` and `worldguard.region.<command>.*` for every `/rg` subcommand
  (`flags` -> `info`, `bypass` -> `toggle-bypass`), checked with `host.has_permission`. A holder runs
  the command on any region (the port's operator path). `worldguard.region.unlimited` lifts claim
  limits. Operators still pass by level: `has_permission` falls back to `op-permission-level`.
- Not done: scoped forms `.own.<region>`, `.member.<region>`, `.<region>`; `worldguard.region.list.own`
  (holders of `list` see all regions); `worldguard.region.flag.regions.*` and `flag.flags.*` (only the
  basic `flag.*`); `locationoverride.*` (the location check exempts whoever counts as admin here);
  wildcard parents such as `worldguard.region.*` or `worldguard.*` (the permission provider decides).
- Not done: protection bypass by node (`worldguard.region.bypass.<world>`). `Guard::bypasses` checks only
  the operator level, because `State::player` has no host to ask. `/rg bypass` therefore answers that
  only operators can bypass for now.
- Not done: `/wg` subcommands are registered for operators only (level 2), so their nodes
  (`worldguard.reload`, `worldguard.flushstates`, ...) are not checked.
- Not read: `op-permissions` (the host always falls back to the operator level, the mod cannot turn that off).
- Read now: `nether-portal-protection` (default true): on `player_portal` with cause `nether_portal`,
  the player must be allowed to build at the destination (x and z times 1/8 or 8, same y). Approximate:
  the real rule looks at the portal it would create; the same place, the same effect in practice.
- Read now: `fake-player-build-override` (default true): names that start with `[` and end with `]`
  bypass protection. Behind a proxy that verifies accounts, names cannot contain
  brackets, so it cannot trigger for real players.
- Read now: `location-flags-only-inside-regions` (default false): a location flag (`teleport`, `spawn`)
  must be inside the region unless the sender counts as admin here. The global region is exempt.
- Guesses (check against the real plugin): the wording of the bypass refusal (`commands.rs`, `bypass`
  arm), the location-flag refusal (`commands.rs` `set_flag`), `holds_node` in `commands.rs`, and the
  portal scaling in `lib.rs` `player_portal`.

## Found in round 3 (2026-11-08)

Missing from the port because Horizon has no hook or the feature is outside the region system:

- `/god`, `/ungod`, `/heal`, `/slay`, `/locate`, `/stack`, `/stopfire`, `/allowfire`, `/halt-activity`,
  `/wg report|profile|stopprofile|debug` (WorldGuard's `GeneralCommands`, `ToggleCommands`, `DebuggingCommands`).
- The per-world options that need hooks: `protection.*` (item durability, infinite stacks), `physics.*`,
  `weather.*`, `crops.*`/`turtle-egg.*` trampling, `chest-protection`, `build-permission-nodes`, the
  blacklist, `simulation.sponge`, `gameplay.block-potions`, `player-damage` types other than fall and mob.
  They are parsed into `Config::rules` where listed, but only the keys named in `config.rs` act.
- `protection.use-max-priority-association` is read but not applied (the query has no config).
- Entity damage of an explosion cannot be cancelled with the block list alone (`explosion-flags-block-entity-damage`
  clears the blocks only).
- `WorldEdit`'s `MessageBox` and `PaginationBox` (the frames of `/rg info`, `list`, `flags`) are not in
  WorldGuard's source; the frames are approximations. Clickable parts are plain text.
- A burning block is checked with the `fire-spread` flag only; WorldGuard checks the break through the fire block's
  regions (the burn hook has no fire source).
- Names that cannot be turned into a UUID are stored as names (WorldGuard refuses: "Unable to resolve the names ...").
- Vanilla spawn protection warnings (`checkSpawnOverlap`) and the `warnAboutSaveFailures` notice.
- Item frames with an item: `item-frame-rotation` needs to know the frame holds an item
  (`Guard::rotate_frame` exists, no hook calls it).
- `receive-chat` per recipient, `time-lock`, `weather-lock`, `heal-*`/`feed-*`, `natural-*` regen flags,
  `potion-splash`, `block-trampling`, `enderman-grief` and friends: see the hook list above.
