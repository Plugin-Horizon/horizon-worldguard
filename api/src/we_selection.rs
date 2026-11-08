//! Selections from the world editing mod (bundle id `worldedit_server`, the server bundle of horizon-worldedit), through the small interface
//! SELECTION.md asks it to publish from its `[api]` package. Built only when that mod is
//! installed (the launcher then enables the `worldedit_server` feature).

use crate::wg::geom::BlockPos;
use crate::wg::selection::{Selection, Selections};
use worldedit_server::selection::{self as we, RegionSelection};

pub struct WorldEdit;

impl Selections for WorldEdit {
    fn get(&self, player: u128) -> Option<Selection> {
        Some(match we::selection_of(player)? {
            RegionSelection::Cuboid { dimension, pos1, pos2 } => Selection::Cuboid {
                dimension,
                a: BlockPos::from(pos1),
                b: BlockPos::from(pos2),
            },
            RegionSelection::Polygon2d { dimension, points, min_y, max_y } => {
                Selection::Polygon { dimension, points, min_y, max_y }
            }
        })
    }

    fn set(&mut self, player: u128, selection: Selection) -> bool {
        let s = match selection {
            Selection::Cuboid { dimension, a, b } => RegionSelection::Cuboid {
                dimension,
                pos1: a.arr(),
                pos2: b.arr(),
            },
            Selection::Polygon { dimension, points, min_y, max_y } => {
                RegionSelection::Polygon2d { dimension, points, min_y, max_y }
            }
        };
        we::set_selection(player, s)
    }
}
