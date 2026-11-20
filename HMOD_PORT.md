# hmod port: worldguard
Parity: 72% now / 72% with our PRs (323/450 items; with the late-node PR, region-scoped nodes count as done: 324/450; method: commands, permission nodes, config keys, flags and listed features of WorldGuard 7.0.22 counted against the original, items blocked by a missing engine API count as not done; round 3 said 71%, its count was about 321/450)

Round 1
- Fixed: `entity_interact` takes `HostSide` and returns `Option<InteractionResult>`; a denial is `InteractionResult::Fail` (src/lib.rs).
- Fixed: `item_toss` event type has no lifetime parameter (`ItemTossEvent`) (src/lib.rs).
- Fixed: `permission_value` replaced by typed `permission_bool` queries in `KnownPlayers::has_permission`, `may` and `may_rg_sub`; `KnownPlayers` now holds the host in a `RefCell<&mut dyn ServerHost>` because permission queries need it mutably (src/lib.rs).
- Fixed: `complete_region`, `complete_wg`, `may`, `may_rg_sub` take `&mut dyn ServerHost`; callers pass `&mut *host` (src/lib.rs).
- Added: `permission_gather_nodes` declares the static nodes (`worldguard.region.<name>` for plugin commands and region flags, `worldguard.flushstates|running|report|reload`, `worldguard.god|heal|slay` and their `.other`), plus `worldguard.region.<sub>`, `.own`, `.member` for every `/rg` subcommand from `commands::usages()` (src/lib.rs).
- Not fixed, unsure: `worldedit::api::{selection, select_cuboid}` (lines 124, 135) are unresolved in the check workspace only. The crate alias `worldedit` is added by the launcher from `horizon-mod.toml`, and the check workspace generator does not add it. No `Cargo.toml` change made (the launcher owns the alias). If the real build still fails, the alias is missing on the build side.
- Permission nodes: static `worldguard.*` nodes listed above, Boolean, default level 2 (declared in `permission_gather_nodes`, src/lib.rs). Helper `fn perm_node(name)`.
- Gaps:
  - `region-scoped-nodes`: `worldguard.region.<perm>.<id>`, `.own.<id>`, `.member.<id>` and `worldguard.region.addowner.unclaimed.<id>` depend on the region id, so they cannot be declared at gather time. They are refused (denial). Effect: non-operators lose per-region grants; region owners can only run `/rg` actions listed under `player-commands`. Protection itself (the guard) does not use these nodes and is unchanged. Marked at `KnownPlayers::has_permission` (src/lib.rs) and at the `addowner.unclaimed` check (src/worldguard_core/commands.rs).
- Unsure:
  - Node defaults are level 2 (config default of `op-permission-level`). A server with another value keeps 2 as the default for the declared nodes; `may` no longer uses the configured level as a fallback (the `_op_level` parameter is unused).
  - `KnownPlayers::op_level` is no longer read (warning only).
  - `entity_interact` cancel is mapped to `InteractionResult::Fail` rather than `Flow::Cancel`.
  - `worldedit` alias: see Fixed/Not fixed above.

Round 2 (parity fixes)
- Fixed: 20 flags that were saved but never read now have handlers (src/lib.rs hooks, src/worldguard_core/guard.rs): `block-trampling` (`entity_change_block_pre`), `crop-growth` (`crop_grow_pre`), `moisture-change`, `lava-harden` (`fluid_place_block`), `exp-drops` (`exp_drop`, `living_experience_drop`), `enderman-grief`, `snowman-trails`, `ravager-grief` (`entity_mob_griefing`), `wind-charge-burst` (`explosion_detonate` entity list), `potion-splash` (lingering clouds in `entity_join_level`), `natural-health-regen` (`living_heal`), `natural-hunger-drain` (`food_level_changed`), `heal-*` and `feed-*` (`vitals.rs`, run from `server_tick`).
- Fixed: `/rg flag` warns for `weather-lock`, `time-lock` and `potion-splash`; the world options `crops.*`, `turtle-egg.*`, `sniffer-egg.*`, `mobs.disable-enderman-griefing`, `mobs.disable-snowman-trails`, `dynamics.disable-crop-growth|soil-moisture-change|lava-harden`, `protection.disable-xp-orb-drops`, `default.disable-health-regain` and the `player-damage.disable-*-damage` options are acted on.
- Fixed: commands `/stopfire`, `/allowfire`, `/halt-activity` (`/stoplag`, `/haltactivity`), `/stack`, `/wg report`; `/god`, `/ungod`, `/heal`, `/slay` use WorldGuard's messages, `-s`, `*` and set health and food directly; an unknown `/wg` word no longer asks for `worldguard.reload`; `/rg flag` completion knows `-w` and `-h`.
- Fixed: `worldguard.region.bypass.<world>` is declared for each world and read once a second (`Guard::bypasses` takes `Player::bypass_perm` as well as the operator level).
- Permission nodes: `worldguard.stack`, `worldguard.profile`, `worldguard.halt-activity`, `worldguard.fire-toggle.stop`, `worldguard.report.pastebin`, `worldguard.debug.event`, `worldguard.region.bypass.<world>` -> default level 2, `permission_gather_nodes`.
- Gaps:
  - HMOD-GAP(player_weather_time): `weather-lock` and `time-lock` need a per-player weather and time; none on Horizon main.
  - HMOD-GAP(potion_splash_cancel): `potion_splash` is listen-only, so splash hits cannot be held back.
  - HMOD-GAP(heal_cause): `living_heal` does not say why; heals of one point or less count as natural regeneration.
  - HMOD-GAP(halt_entities): `/halt-activity` cannot list the loaded entities to remove them.
  - HMOD-GAP(compass_target) `/locate`; HMOD-GAP(profiler) `/wg profile|stopprofile|debug`.
- Unsure: `PlayerRef::food`, `set_food_level`, `set_saturation`, `set_exhaustion`, `inventory_slots`, `set_inventory_slot`, `ItemKey::max_stack_size` and `BlockState::block().id(host)` are used by the naming convention of the generated handle methods, not checked against a build.

Round 3 (100 % round, written blind: nothing compiled or run)
- Added: per-world config files. `<data-folder>/worlds/<world>/config.yml` (WorldGuard's layout, any section of `config/worldguard-common.toml`) overrides the common file for that world; read with the regions at start and on `/wg reload` and `/rg load`. `Config::rule_in|list_in|int_in`, `Config::set_world_yaml` (`config.rs`), `Guard::rule(dimension, key)` (every world option in `guard.rs` now reads through it).
- Added: world options with a list or number value: `mobs.block-creature-spawn`, `physics.disable-water-damage-blocks`, `fire.disable-fire-spread-blocks`, `fire.lava-spread-blocks`, `dynamics.snow-fall-blocks`, `weather.prevent-lightning-strike-blocks`, `simulation.sponge.radius` (`Config::lists`/`ints`; nested tables of the common file are flattened to dotted keys).
- Added: weather rules. `weather.disable-weather|disable-thunderstorm|always-raining|always-thundering` (`weather_change`, the last allowed state per dimension is kept to tell what changed), `disable-lightning-strike-fire` (`Guard::ignite`), `prevent-lightning-strike-blocks` and the `lightning` flag (a lightning bolt joining the level), `disable-pig-zombification|villager-witchification|powered-creepers` (`entity_struck_by_lightning`: a cancel also spares the animal the bolt's damage).
- Added: mob rules. `mobs.block-above-ground-slimes` (slimes from y 60 up), `block-painting-destroy`, `block-item-frame-destroy` (breaks without a player), `block-armor-stand-destroy`, `block-vehicle-entry`.
- Added: physics and fire. `physics.disable-water-damage-blocks` and `fire.lava-spread-blocks` in `block_spread`; the fluid is now told from the block it flows out of, so `water-flow` and `lava-flow` apply to their own fluid (closes that Known gap). `fire.disable-fire-spread-blocks` (neighbours of the new fire, and burning of those blocks), `ignition.block-lighter` (operators stand in for `worldguard.override.lighter`).
- Added: `player-damage.teleport-on-void-falling` and `teleport-on-suffocation`: the damage is cancelled and the player is moved to where they were 3-4 seconds earlier (one sample a second). `reset-fall-on-void-teleport` is read but a teleport is the only reset (see gaps).
- Added: hooks for the block flags: `block_fade` (`ice-melt`, `snow-melt`, `soil-dry`, `coral-fade`, `copper-fade`, the matching `dynamics.*` options), `block_form` (`ice-form`, `snow-fall`, `frosted-ice-form`), `block_grow_feature` (`mushroom-growth`).
- Added: blacklist (`worldguard_core/blacklist.rs`). `worlds/<world>/blacklist.txt` in WorldGuard's format (`[a,b]` headings, `on-break|on-place|on-interact|on-destroy-with|on-acquire|on-drop|on-use=deny,allow,tell,notify,log,kick,ban`, `message`, `comment`, `ignore-groups`, `ignore-perms`), whitelist mode (`blacklist.use-as-whitelist`), the 3 s repeat guard, console logging (`blacklist.logging.console.enable`). Wired to `break_block`, `entity_place`, `right_click_block`, `item_toss`, `item_picked_up` (silent).
- Added: sign chest protection (`worldguard_core/chest.rs`, `chest_world.rs`): `[Lock]` signs, break/use/place/explosion protection, the `sign_changed` rules and `chest-protection.disable-off-check`. Nodes `worldguard.chest-protection.override` and `worldguard.override.chest-protection`.
- Added: sponges (`simulation.sponge.*`): water does not flow into a sponge's reach; a placed sponge clears the water around it at the next tick (`fill ... replace water`).
- Permission nodes: `worldguard.notify`, `worldguard.chest-protection.override`, `worldguard.override.chest-protection`, `worldguard.override.lighter` -> level 2, `permission_gather_nodes`.
- Gaps (all in GAPS_UPSTREAM.md):
  - HMOD-GAP(block_physics): `physics.no-physics-sand|gravel`, `physics.allow-portal-anywhere`, `physics.vine-like-rope-ladders` need a cancellable block physics hook.
  - HMOD-GAP(zombie_door): `mobs.block-zombie-door-destruction` (`entity_break_door` is listen-only), `mobs.block-enderdragon-portal-creation` (`portal_create` is listen-only), `mobs.anti-wolf-dumbness` (no tamed read).
  - HMOD-GAP(growth_hooks): `grass-growth`, `mycelium-spread`, `vine-growth`, `rock-growth`, `sculk-growth` have no hook (`block_spread` only names fluid and fire).
  - HMOD-GAP(blacklist_equip_dispense): `on-equip` and `on-dispense` need the item of an equip and a dispenser's stack; `blacklist.logging.file|database` are not written; `ignore-groups` needs the player's groups (none are read).
  - HMOD-GAP(death_message): `player-damage.disable-death-messages`, `default.pumpkin-scuba`, `gameplay.disable-conduit-effects`, `protection.item-durability`, `protection.remove-infinite-stacks`, `gameplay.block-potions` have no hook that can change them.
  - HMOD-GAP(void_spawn): `teleport-on-void-falling` and `teleport-on-suffocation` use the recent trail of the player, not the world spawn or a computed free position; the fall distance cannot be reset.
  - HMOD-GAP(damage_dimension): `LivingIncomingDamageEvent` and `HangingBreakEvent` name no dimension, so `mobs.block-armor-stand-destroy`, `block-painting-destroy` and `block-item-frame-destroy` read the overworld's options.
  - HMOD-GAP(redstone_sponge): a redstone-powered sponge's toggling is not simulated (only absorption by placement and flow).
- Unsure: `LevelHost::state_block`, `has_neighbor_signal`, `HostReader` (`block_entity` + `entity.data` as the chest shop mod reads sign text), the `EntityStruckByLightningEvent`/`EntityJoinLevelEvent` handles and `Hand::Main` on `hand_item` follow the generated reference and other mods; `weather_change`'s `raining`/`thundering` are taken as the new state, and the dimension string as `minecraft:overworld`.
- Not applicable: `iconomy.*`, `summary-on-start`, `custom-metrics-charts`, `regions.use-paper-entity-origin`, `event-handling.*` (Bukkit event plumbing), SQL region storage.

Round 4 (100 % round, written blind: nothing compiled or run; audit source `audit/worldguard.md`)
- Added: blacklist file log (`blacklist.logging.file.enable`): each line the blacklist logs also goes to a daily file, `<data-folder>/worldguard/logs/%Y-%m-%d.log` (WorldGuard's default pattern; `%u %w %Y %m %d %W %H %h %i %s %%`, unknown codes `?`). Line format `[yyyy-MM-dd HH:mm:ss] <name>: <message>` CRLF (`worldguard_core/blacklist.rs` `log_file_path`, `log_file_line`, `stamp`; `append_log_line` in `lib.rs`). Times are UTC (the original uses the server's time zone). `blacklist.logging.file.path` and `open-files` are not read (the config keeps no text values); each line opens and appends the file (no open-file cache).
- Added: region-scoped permission nodes (`worldguard.region.<perm>.<id>`, `.own.`, `.member.`, `flag.flags.*`, `addowner.unclaimed.<id>`) are declared the first time they are asked, behind the cargo feature `horizon_late_permission_nodes` (off by default). It needs `permission_declare_node` of horizon-rust branch `api-permission_late_node_registration` (issue #18807), which the pinned SDK does not have (`has_permission` in `KnownPlayers`, `declare_region_node_once` in `lib.rs`).
- Fixed: `/wg version` prints `WorldGuard 7.0.22-SNAPSHOT` (was an older plugin version string).
- Checked and already matching (round 2/3): `/rg flag` with `-e`, `-g`, `-w`, `-h <page>`; `/god`, `/ungod`, `/heal`, `/slay` messages, `-s`, `*`, `.other`; `/wg report`; unknown `/wg` words open to everybody.
- Permission nodes: `worldguard.region.<perm>.<id>` family (feature only); the rest unchanged (level 2 default).

Checklist round 100
[x] `/rg` subcommands, aliases and flag options (`-e -g -w -h`) present (commands.rs SPECS, option_spec)
[x] `/rg flag` and `/rg flags` help paging, `-w` on the list commands
[x] `/god /ungod /heal /slay` messages, `-s`, `*`, `.other` nodes (lib.rs god/heal branch)
[x] `/wg reload|flushstates|report|version|running` present; `/wg version` string corrected
[x] `worldguard.*` nodes declared at gather time (round 1-3 lists)
[x] `worldguard.region.bypass.<world>` declared for each world and read (round 2)
[x] blacklist file, console and whitelist-mode logging (round 3, file added round 4)
[x] region-scoped nodes behind `horizon_late_permission_nodes` (round 4)
[ ] `worldguard.region.<perm>.<id>` without the feature: refused (API-blocked, late node PR)
[ ] `/locate`, `worldguard.locate`: no compass target API (HMOD-GAP(compass_target))
[ ] `/wg profile`, `stopprofile`, `debug`: no profiler or event replay (HMOD-GAP(profiler))
[ ] `/wg running` lists tasks: the port runs no async tasks, so it always says none
[ ] `/rg migratedb` only the file store; `migrateuuid` is a stub (no SQL, no UUID migration)
[ ] blacklist `on-equip`, `on-dispense`, `ignore-groups`, database log (API gaps, GAPS_UPSTREAM)
[ ] blacklist `path` and `open-files` text options (config keeps no text values)
[ ] per-world `config.yml` text values (`path`, `dsn`-type options), only booleans, numbers and lists are kept
[ ] `physics.*`, `mobs.block-zombie-door-destruction`, `enderman` portal, `anti-wolf-dumbness`, growth flags (API gaps)
[ ] death messages, pumpkin scuba, conduit effects, item durability, infinite stacks, potion blocking (API gaps)
[ ] `/tp`, portal and splash refusal, vehicle leave (API gaps)
[ ] `weather-lock`, `time-lock` (per-player weather and time, API gap)
[ ] `iconomy.*`, `summary-on-start`, `custom-metrics-charts`, `regions.use-paper-entity-origin`, `event-handling.*`: not applicable
Open items are API-blocked or need a config text store; the checklist is not finished, so this is not 100 %.
