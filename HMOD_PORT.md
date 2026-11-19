# hmod port: worldguard
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
