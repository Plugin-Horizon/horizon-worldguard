//! Region shapes: cuboids, 2D polygons with a height range, and the global region.
//! Block coordinates are inclusive on both ends, as in region files (`min`/`max` are the
//! corner blocks themselves).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
    /// The block that holds the point `p` (feet position of an entity).
    pub fn of(p: [f64; 3]) -> Self {
        Self::new(p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32)
    }
    pub fn arr(self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }
}

impl From<[i32; 3]> for BlockPos {
    fn from(a: [i32; 3]) -> Self {
        Self::new(a[0], a[1], a[2])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Cuboid { min: BlockPos, max: BlockPos },
    Polygon { points: Vec<[i32; 2]>, min_y: i32, max_y: i32 },
    /// The world's `__global__` region, and template regions made with `define -g`: no area.
    Global,
}

impl Shape {
    /// A cuboid from two corners in any order.
    pub fn cuboid(a: BlockPos, b: BlockPos) -> Self {
        Shape::Cuboid {
            min: BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)),
            max: BlockPos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)),
        }
    }

    pub fn polygon(points: Vec<[i32; 2]>, y1: i32, y2: i32) -> Self {
        Shape::Polygon {
            points,
            min_y: y1.min(y2),
            max_y: y1.max(y2),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Shape::Cuboid { .. } => "cuboid",
            Shape::Polygon { .. } => "poly2d",
            Shape::Global => "global",
        }
    }

    /// The bounding box, None for no area.
    pub fn bounds(&self) -> Option<(BlockPos, BlockPos)> {
        match self {
            Shape::Cuboid { min, max } => Some((*min, *max)),
            Shape::Polygon { points, min_y, max_y } if !points.is_empty() => {
                let (mut x0, mut z0, mut x1, mut z1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
                for p in points {
                    x0 = x0.min(p[0]);
                    z0 = z0.min(p[1]);
                    x1 = x1.max(p[0]);
                    z1 = z1.max(p[1]);
                }
                Some((BlockPos::new(x0, *min_y, z0), BlockPos::new(x1, *max_y, z1)))
            }
            _ => None,
        }
    }

    pub fn contains(&self, p: BlockPos) -> bool {
        match self {
            Shape::Cuboid { min, max } => {
                p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y && p.z >= min.z
                    && p.z <= max.z
            }
            Shape::Polygon { points, min_y, max_y } => {
                p.y >= *min_y && p.y <= *max_y && polygon_contains(points, p.x, p.z)
            }
            Shape::Global => false,
        }
    }

    /// The number of blocks inside.
    pub fn volume(&self) -> i64 {
        match self {
            Shape::Cuboid { min, max } => {
                (max.x as i64 - min.x as i64 + 1)
                    * (max.y as i64 - min.y as i64 + 1)
                    * (max.z as i64 - min.z as i64 + 1)
            }
            Shape::Polygon { points, min_y, max_y } => {
                let Some((lo, hi)) = self.bounds() else {
                    return 0;
                };
                let mut area: i64 = 0;
                // Column count by scanning: exact for the inclusive edge rule.
                if (hi.x as i64 - lo.x as i64 + 1) * (hi.z as i64 - lo.z as i64 + 1) <= 4_000_000 {
                    for x in lo.x..=hi.x {
                        for z in lo.z..=hi.z {
                            if polygon_contains(points, x, z) {
                                area += 1;
                            }
                        }
                    }
                } else {
                    area = shoelace(points).round() as i64;
                }
                area * (*max_y as i64 - *min_y as i64 + 1)
            }
            Shape::Global => 0,
        }
    }

    /// Whether the two shapes share at least one block.
    pub fn intersects(&self, other: &Shape) -> bool {
        let (Some((a0, a1)), Some((b0, b1))) = (self.bounds(), other.bounds()) else {
            return false;
        };
        if a1.x < b0.x || b1.x < a0.x || a1.y < b0.y || b1.y < a0.y || a1.z < b0.z || b1.z < a0.z
        {
            return false;
        }
        if let (Shape::Cuboid { .. }, Shape::Cuboid { .. }) = (self, other) {
            return true;
        }
        // Scan the overlap of the boxes column by column (bounded; region shapes are small).
        let (x0, x1) = (a0.x.max(b0.x), a1.x.min(b1.x));
        let (z0, z1) = (a0.z.max(b0.z), a1.z.min(b1.z));
        let y = a0.y.max(b0.y);
        let cols = (x1 as i64 - x0 as i64 + 1) * (z1 as i64 - z0 as i64 + 1);
        if cols > 16_000_000 {
            return true; // too big to scan; boxes overlap, assume the shapes do
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                let p = BlockPos::new(x, y, z);
                if self.contains(p) && other.contains(p) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether `other` lies completely inside this shape (checked on its corner/vertex columns
    /// and edges, which is exact for cuboids and close for polygons).
    pub fn contains_shape(&self, other: &Shape) -> bool {
        let Some((lo, hi)) = other.bounds() else {
            return false;
        };
        let pts: Vec<[i32; 2]> = match other {
            Shape::Cuboid { .. } => vec![[lo.x, lo.z], [hi.x, lo.z], [hi.x, hi.z], [lo.x, hi.z]],
            Shape::Polygon { points, .. } => points.clone(),
            Shape::Global => return false,
        };
        let edge_ok = |a: [i32; 2], b: [i32; 2]| {
            let steps = (b[0] - a[0]).abs().max((b[1] - a[1]).abs()).max(1);
            (0..=steps).all(|i| {
                let x = a[0] + (b[0] - a[0]) * i / steps;
                let z = a[1] + (b[1] - a[1]) * i / steps;
                self.contains(BlockPos::new(x, lo.y, z)) && self.contains(BlockPos::new(x, hi.y, z))
            })
        };
        (0..pts.len()).all(|i| edge_ok(pts[i], pts[(i + 1) % pts.len()]))
    }

    /// The centre block of the bounding box (for `/rg tp -c`).
    pub fn center(&self) -> Option<[f64; 3]> {
        let (lo, hi) = self.bounds()?;
        Some([
            (lo.x as f64 + hi.x as f64 + 1.0) / 2.0,
            (lo.y as f64 + hi.y as f64 + 1.0) / 2.0,
            (lo.z as f64 + hi.z as f64 + 1.0) / 2.0,
        ])
    }
}

/// Point in polygon on block columns. Points on an edge or vertex count as inside, so a polygon
/// drawn through blocks includes those blocks.
pub fn polygon_contains(points: &[[i32; 2]], x: i32, z: i32) -> bool {
    let n = points.len();
    if n < 3 {
        return n > 0 && points.iter().any(|p| p[0] == x && p[1] == z)
            || (n == 2 && on_segment(points[0], points[1], x, z));
    }
    let mut inside = false;
    let (px, pz) = (x as i64, z as i64);
    for i in 0..n {
        let a = points[i];
        let b = points[(i + n - 1) % n];
        if on_segment(a, b, x, z) {
            return true;
        }
        let (ax, az, bx, bz) = (a[0] as i64, a[1] as i64, b[0] as i64, b[1] as i64);
        if (az > pz) != (bz > pz) {
            // x of the edge at height pz, compared without division.
            let lhs = (px - ax) * (bz - az);
            let rhs = (bx - ax) * (pz - az);
            if (bz - az > 0 && lhs < rhs) || (bz - az < 0 && lhs > rhs) {
                inside = !inside;
            }
        }
    }
    inside
}

fn on_segment(a: [i32; 2], b: [i32; 2], x: i32, z: i32) -> bool {
    let (ax, az, bx, bz, px, pz) = (
        a[0] as i64, a[1] as i64, b[0] as i64, b[1] as i64, x as i64, z as i64,
    );
    let cross = (bx - ax) * (pz - az) - (bz - az) * (px - ax);
    cross == 0 && px >= ax.min(bx) && px <= ax.max(bx) && pz >= az.min(bz) && pz <= az.max(bz)
}

fn shoelace(points: &[[i32; 2]]) -> f64 {
    let n = points.len();
    let mut s = 0i64;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        s += a[0] as i64 * b[1] as i64 - b[0] as i64 * a[1] as i64;
    }
    (s as f64 / 2.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuboid_inclusive() {
        let c = Shape::cuboid(BlockPos::new(5, 70, 5), BlockPos::new(-5, 60, -5));
        assert!(c.contains(BlockPos::new(-5, 60, 5)));
        assert!(c.contains(BlockPos::new(5, 70, -5)));
        assert!(!c.contains(BlockPos::new(6, 65, 0)));
        assert_eq!(c.volume(), 11 * 11 * 11);
    }

    #[test]
    fn polygon_edges_and_inside() {
        // An L shape.
        let p = Shape::polygon(vec![[0, 0], [10, 0], [10, 4], [4, 4], [4, 10], [0, 10]], 0, 10);
        assert!(p.contains(BlockPos::new(0, 5, 0)));
        assert!(p.contains(BlockPos::new(10, 5, 4)));
        assert!(p.contains(BlockPos::new(2, 5, 8)));
        assert!(!p.contains(BlockPos::new(8, 5, 8)));
        assert!(!p.contains(BlockPos::new(2, 11, 2)));
        // 11*5 + 5*6 columns, 11 high.
        assert_eq!(p.volume(), (11 * 5 + 5 * 6) * 11);
    }

    #[test]
    fn intersections() {
        let a = Shape::cuboid(BlockPos::new(0, 0, 0), BlockPos::new(10, 10, 10));
        let b = Shape::cuboid(BlockPos::new(10, 10, 10), BlockPos::new(20, 20, 20));
        let c = Shape::cuboid(BlockPos::new(11, 0, 0), BlockPos::new(20, 10, 10));
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
        let tri = Shape::polygon(vec![[0, 0], [10, 0], [0, 10]], 0, 5);
        let corner = Shape::cuboid(BlockPos::new(8, 0, 8), BlockPos::new(10, 5, 10));
        assert!(!tri.intersects(&corner));
        assert!(a.contains_shape(&Shape::cuboid(BlockPos::new(1, 1, 1), BlockPos::new(9, 9, 9))));
        assert!(!a.contains_shape(&b));
    }
}
