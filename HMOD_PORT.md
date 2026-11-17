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
