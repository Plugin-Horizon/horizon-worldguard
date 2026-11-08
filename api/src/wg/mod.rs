//! The engine-free part of the WorldGuard port: region model, region files, flag queries,
//! commands and protection rules. No Horizon types here, so it builds and tests on its own
//! (`core/Cargo.toml` points at this file).
//!
//! Modules refer to each other through `super::`, so this file works both as a crate root
//! (the test crate) and as the `wg` module of the trait mod in `api/`.

pub mod flags;
pub mod geom;
pub mod query;
pub mod region;
pub mod store;
pub mod yaml;
pub mod config;
pub mod guard;
pub mod kinds;
pub mod text;
pub mod commands;
pub mod movement;
pub mod selection;
