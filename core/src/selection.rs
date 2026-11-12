//! The selection a region command works on. Selections belong to the world editing mod; this is
//! the minimal shape the region commands need from it (SELECTION.md).

use super::geom::{BlockPos, Shape};

/// A player's selection in one world (dimension id, `minecraft:overworld`).
#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    Cuboid { dimension: String, a: BlockPos, b: BlockPos },
    Polygon { dimension: String, points: Vec<[i32; 2]>, min_y: i32, max_y: i32 },
}

impl Selection {
    pub fn dimension(&self) -> &str {
        match self {
            Selection::Cuboid { dimension, .. } | Selection::Polygon { dimension, .. } => dimension,
        }
    }

    pub fn shape(&self) -> Result<Shape, String> {
        match self {
            Selection::Cuboid { a, b, .. } => Ok(Shape::cuboid(*a, *b)),
            Selection::Polygon { points, min_y, max_y, .. } => {
                if points.len() < 3 {
                    return Err("A polygon selection needs at least three points.".into());
                }
                Ok(Shape::polygon(points.clone(), *min_y, *max_y))
            }
        }
    }

    /// The selection of a region's shape, for `/rg select`.
    pub fn of_shape(dimension: &str, shape: &Shape) -> Option<Selection> {
        match shape {
            Shape::Cuboid { min, max } => Some(Selection::Cuboid {
                dimension: dimension.to_owned(),
                a: *min,
                b: *max,
            }),
            Shape::Polygon { points, min_y, max_y } => Some(Selection::Polygon {
                dimension: dimension.to_owned(),
                points: points.clone(),
                min_y: *min_y,
                max_y: *max_y,
            }),
            Shape::Global => None,
        }
    }
}

/// Where selections come from. The trait mod implements it over the world editing mod when that
/// is installed; tests use a map.
pub trait Selections {
    /// The player's current selection, None when they have none (or no editing mod is there).
    fn get(&self, player: u128) -> Option<Selection>;
    /// Replaces the player's selection; false when that is not possible.
    fn set(&mut self, player: u128, selection: Selection) -> bool;
}

/// A plain store (tests, and the fallback when no editing mod is installed).
#[derive(Default)]
pub struct Store(pub std::collections::HashMap<u128, Selection>);

impl Selections for Store {
    fn get(&self, player: u128) -> Option<Selection> {
        self.0.get(&player).cloned()
    }
    fn set(&mut self, player: u128, selection: Selection) -> bool {
        self.0.insert(player, selection);
        true
    }
}
