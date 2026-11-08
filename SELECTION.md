# Selection interface

`/rg define`, `redefine`, `claim`, `list -s` and `select`/`info -s` work on a player's selection.
Selections belong to the world editing mod. This mod does not keep its own; it reads and writes
them through two functions the editing mod publishes from its `[api]` package (trait mod
dependencies, docs/native-mods.md "Dependencies between trait mods").

## What the world editing mod publishes

Bundle id `worldedit_server` (horizon-worldedit's server bundle), `[api]` package with a public module `selection`:

```rust
pub mod selection {
    /// A player's selection. `dimension` is the level id (`minecraft:overworld`).
    /// Corners and points are block coordinates, both ends inclusive.
    #[derive(Clone, Debug, PartialEq)]
    pub enum RegionSelection {
        /// The two clicked corners in click order (pos1, pos2), not sorted.
        Cuboid { dimension: String, pos1: [i32; 3], pos2: [i32; 3] },
        /// A polygon selection: x/z points in order, and its height range.
        Polygon2d { dimension: String, points: Vec<[i32; 2]>, min_y: i32, max_y: i32 },
    }

    /// The player's current selection; None when they have none or it is incomplete.
    pub fn selection_of(player: u128) -> Option<RegionSelection>;

    /// Replaces the player's selection (and shows it as the editing mod usually does).
    /// false when the player is unknown or the selection type is not supported.
    pub fn set_selection(player: u128, selection: RegionSelection) -> bool;
}
```

- Both must be callable from any server hook (the state behind them synchronised, as the docs
  require for public statics).
- Other selection types (ellipsoid, cylinder, convex) are simply not returned: `selection_of`
  answers None, or the editing mod may return their bounding cuboid.
- Nothing else is needed. The region mod never edits blocks through the editing mod.

## This side

`horizon-mixin.toml` declares `worldedit_server` as an optional dependency, so the launcher enables the
`worldedit_server` feature only when that mod is installed. `api/src/we_selection.rs` adapts the two
functions to `wg::selection::Selections`; without the mod, a plain in-memory store is used and the
selection commands say that there is no selection.
