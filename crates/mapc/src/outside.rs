//! The void outside a sealed map: never seen, so faces facing it are
//! dropped. Found by flooding the map's open space from its point
//! entities (which stand inside) on a voxel grid:
//!
//! ```text
//!   ┌───────────────────┐   ░ flooded from the light ●
//!   │░░░░░░░░░░░░░░░░░░░│   faces with flooded space in front
//!   │░░░░░●░░░░░░░░░░░░░│   are kept; the outside faces of the
//!   │░░░░░░░░░░░░░░░░░░░│   walls see only unflooded void and
//!   └───────────────────┘   are dropped
//! ```
//!
//! A voxel is solid when all eight corners are inside sealing brushes
//! (solid or sky), so open space is never closed off, and walls at
//! least two voxels thick always seal. If the flood reaches the grid's
//! edge the map leaks: a gap, or an entity outside. Then nothing is
//! dropped, as a leaking qbsp compile drops nothing.
//!
//! Gaps narrower than a voxel count as solid, so faces inside such
//! cracks go too; nobody can see into them.

use std::collections::VecDeque;

use glam::{DVec3, I64Vec3};
use level::Contents;

use crate::solid::Solid;
use crate::winding::Winding;

/// Voxel side, map units. Walls of 8 units seal.
const VOXEL: f64 = 4.0;
/// Empty voxels around the map's bounds, where a leak shows.
const PAD: i64 = 1;
/// Samples sit this far in front of a face, map units: inside the
/// voxel next to it.
const SAMPLE_OFFSET: f64 = 1.0;
/// Samples are pulled this fraction from each corner toward the
/// center, off the edges.
const CORNER_INSET: f64 = 0.1;
/// A point this close to a brush plane counts as inside it.
const ON_EPSILON: f64 = 0.01;
const NEIGHBORS: [I64Vec3; 6] = [
    I64Vec3::X,
    I64Vec3::NEG_X,
    I64Vec3::Y,
    I64Vec3::NEG_Y,
    I64Vec3::Z,
    I64Vec3::NEG_Z,
];

/// Why the outside could not be found.
#[derive(Debug, PartialEq, thiserror::Error)]
pub(crate) enum Unsealed {
    /// No point entity stands in open space to flood from.
    #[error("no point entity inside the map to flood from")]
    NoSeeds,
    /// The flood from this entity reached the outside.
    #[error("the map leaks: open space at {from} reaches the outside")]
    Leak {
        /// The entity origin the flood started from.
        from: DVec3,
    },
}

/// The open space reachable from inside a sealed map.
pub(crate) struct Interior {
    grid: Grid,
    reached: Vec<bool>,
}

impl Interior {
    /// Floods from `seeds` (point entity origins) around `solids`.
    pub(crate) fn flood(solids: &[Solid], seeds: &[DVec3]) -> Result<Self, Unsealed> {
        let sealing: Vec<&Solid> = solids.iter().filter(|s| seals(s.contents)).collect();
        let Some(grid) = Grid::around(&sealing) else {
            return Err(Unsealed::NoSeeds);
        };
        let solid = grid.solid_voxels(&sealing);

        let mut reached = vec![false; grid.len()];
        let mut flooded_any = false;
        for &seed in seeds {
            let Some(start) = grid.voxel(seed) else {
                return Err(Unsealed::Leak { from: seed });
            };
            if solid[grid.index(start)] || reached[grid.index(start)] {
                continue;
            }

            flooded_any = true;
            if grid.flood(start, &solid, &mut reached) == Flood::Leaked {
                return Err(Unsealed::Leak { from: seed });
            }
        }

        match flooded_any {
            true => Ok(Self { grid, reached }),
            false => Err(Unsealed::NoSeeds),
        }
    }

    /// Whether open space in front of `winding` (facing `normal`) is
    /// reachable from inside.
    pub(crate) fn sees(&self, winding: &Winding, normal: DVec3) -> bool {
        let points = winding.points();
        let center = points.iter().sum::<DVec3>() / points.len() as f64;
        let insets = points.iter().map(|&p| p + (center - p) * CORNER_INSET);

        std::iter::once(center)
            .chain(insets)
            .filter_map(|p| self.grid.voxel(p + normal * SAMPLE_OFFSET))
            .any(|v| self.reached[self.grid.index(v)])
    }
}

/// Brushes that close off space: those that block sight. Liquids and
/// clip do not.
fn seals(contents: Contents) -> bool {
    matches!(contents, Contents::Solid | Contents::Sky)
}

#[derive(PartialEq)]
enum Flood {
    Sealed,
    Leaked,
}

/// A box of voxels over the map, padded on every side.
struct Grid {
    /// Corner of voxel (0, 0, 0).
    origin: DVec3,
    /// Voxels along each axis.
    dims: I64Vec3,
}

impl Grid {
    /// Covers every solid's bounds plus padding; None for no solids.
    fn around(solids: &[&Solid]) -> Option<Self> {
        let first = solids.first()?;
        let (mins, maxs) = solids.iter().fold((first.mins, first.maxs), |(lo, hi), s| {
            (lo.min(s.mins), hi.max(s.maxs))
        });

        let origin = (mins / VOXEL).floor() * VOXEL - PAD as f64 * VOXEL;
        let dims = ((maxs - origin) / VOXEL).ceil().as_i64vec3() + PAD;
        Some(Self { origin, dims })
    }

    fn len(&self) -> usize {
        (self.dims.x * self.dims.y * self.dims.z) as usize
    }

    fn index(&self, v: I64Vec3) -> usize {
        (v.x + self.dims.x * (v.y + self.dims.y * v.z)) as usize
    }

    /// The voxel holding `p`; None outside the grid.
    fn voxel(&self, p: DVec3) -> Option<I64Vec3> {
        let v = ((p - self.origin) / VOXEL).floor().as_i64vec3();
        let inside = v.cmpge(I64Vec3::ZERO).all() && v.cmplt(self.dims).all();
        inside.then_some(v)
    }

    /// Whether `v` is in the outermost layer, past the map.
    fn on_edge(&self, v: I64Vec3) -> bool {
        v.cmpeq(I64Vec3::ZERO).any() || v.cmpeq(self.dims - 1).any()
    }

    /// Solid voxels: every corner inside some brush. Corners are
    /// marked brush by brush, over each brush's own bounds.
    fn solid_voxels(&self, solids: &[&Solid]) -> Vec<bool> {
        let corners = self.dims + 1;
        let corner_index = |c: I64Vec3| (c.x + corners.x * (c.y + corners.y * c.z)) as usize;
        let mut inside = vec![false; (corners.x * corners.y * corners.z) as usize];

        for solid in solids {
            let lo = ((solid.mins - self.origin) / VOXEL)
                .floor()
                .as_i64vec3()
                .max(I64Vec3::ZERO);
            let hi = ((solid.maxs - self.origin) / VOXEL)
                .ceil()
                .as_i64vec3()
                .min(self.dims);
            for z in lo.z..=hi.z {
                for y in lo.y..=hi.y {
                    for x in lo.x..=hi.x {
                        let c = I64Vec3::new(x, y, z);
                        let p = self.origin + c.as_dvec3() * VOXEL;
                        if contains(solid, p) {
                            inside[corner_index(c)] = true;
                        }
                    }
                }
            }
        }

        let offsets: Vec<I64Vec3> = (0..8)
            .map(|i| I64Vec3::new(i & 1, (i >> 1) & 1, (i >> 2) & 1))
            .collect();
        let mut solid = vec![false; self.len()];
        for z in 0..self.dims.z {
            for y in 0..self.dims.y {
                for x in 0..self.dims.x {
                    let v = I64Vec3::new(x, y, z);
                    solid[self.index(v)] = offsets.iter().all(|&o| inside[corner_index(v + o)]);
                }
            }
        }
        solid
    }

    /// Marks open voxels reachable from `start`; stops at the edge.
    fn flood(&self, start: I64Vec3, solid: &[bool], reached: &mut [bool]) -> Flood {
        let mut queue = VecDeque::from([start]);
        reached[self.index(start)] = true;

        while let Some(v) = queue.pop_front() {
            if self.on_edge(v) {
                return Flood::Leaked;
            }
            for step in NEIGHBORS {
                let next = v + step;
                let i = self.index(next);
                if solid[i] || reached[i] {
                    continue;
                }
                reached[i] = true;
                queue.push_back(next);
            }
        }

        Flood::Sealed
    }
}

/// Whether `p` is inside `solid` or on its surface.
fn contains(solid: &Solid, p: DVec3) -> bool {
    solid
        .sides
        .iter()
        .all(|s| s.plane.distance(p) <= ON_EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::cuboid;

    /// A sealed 128-unit room with 16-unit walls: brushes for the floor,
    /// ceiling and four walls. `gap` leaves out the east wall.
    fn room(gap: Gap) -> Vec<mapfile::Brush> {
        let (lo, hi) = (-16.0, 144.0);
        let mut brushes = vec![
            cuboid(DVec3::new(lo, lo, lo), DVec3::new(hi, hi, 0.0), "floor"),
            cuboid(DVec3::new(lo, lo, 128.0), DVec3::new(hi, hi, hi), "floor"),
            cuboid(DVec3::new(lo, lo, 0.0), DVec3::new(0.0, hi, 128.0), "floor"),
            cuboid(DVec3::new(lo, lo, 0.0), DVec3::new(hi, 0.0, 128.0), "floor"),
            cuboid(
                DVec3::new(lo, 128.0, 0.0),
                DVec3::new(hi, hi, 128.0),
                "floor",
            ),
        ];
        if gap == Gap::Closed {
            brushes.push(cuboid(
                DVec3::new(128.0, lo, 0.0),
                DVec3::new(hi, hi, 128.0),
                "floor",
            ));
        }
        brushes
    }

    #[derive(PartialEq)]
    enum Gap {
        Open,
        Closed,
    }

    fn solids(brushes: &[mapfile::Brush]) -> Vec<Solid<'_>> {
        brushes.iter().map(|b| Solid::build(b).unwrap()).collect()
    }

    /// The side of `solid` facing `normal`.
    fn side<'a>(solid: &'a Solid, normal: DVec3) -> &'a crate::solid::Side<'a> {
        solid
            .sides
            .iter()
            .find(|s| s.plane.normal == normal)
            .unwrap()
    }

    #[test]
    fn a_sealed_room_sees_its_inner_faces_only() {
        let brushes = room(Gap::Closed);
        let solids = solids(&brushes);
        let interior = Interior::flood(&solids, &[DVec3::splat(64.0)]).unwrap();

        let floor_top = side(&solids[0], DVec3::Z);
        let floor_bottom = side(&solids[0], DVec3::NEG_Z);
        let west_inner = side(&solids[2], DVec3::X);
        let west_outer = side(&solids[2], DVec3::NEG_X);

        assert!(interior.sees(&floor_top.winding, DVec3::Z));
        assert!(interior.sees(&west_inner.winding, DVec3::X));
        assert!(!interior.sees(&floor_bottom.winding, DVec3::NEG_Z));
        assert!(!interior.sees(&west_outer.winding, DVec3::NEG_X));
    }

    #[test]
    fn a_leak_or_no_seed_seals_nothing() {
        let open = room(Gap::Open);
        let from = DVec3::splat(64.0);
        assert_eq!(
            Interior::flood(&solids(&open), &[from]).err(),
            Some(Unsealed::Leak { from })
        );

        let closed = room(Gap::Closed);
        let solids = solids(&closed);
        assert_eq!(Interior::flood(&solids, &[]).err(), Some(Unsealed::NoSeeds));

        let in_wall = DVec3::new(-8.0, 64.0, 64.0);
        assert_eq!(
            Interior::flood(&solids, &[in_wall]).err(),
            Some(Unsealed::NoSeeds)
        );

        let outside = DVec3::splat(1000.0);
        let leak = Unsealed::Leak { from: outside };
        assert_eq!(Interior::flood(&solids, &[outside]).err(), Some(leak));
    }

    #[test]
    fn liquids_do_not_seal() {
        let mut brushes = room(Gap::Closed);
        brushes[1] = cuboid(
            DVec3::new(-16.0, -16.0, 128.0),
            DVec3::splat(144.0),
            "*water",
        );
        let from = DVec3::splat(64.0);

        let leak = Interior::flood(&solids(&brushes), &[from]).err();
        assert_eq!(leak, Some(Unsealed::Leak { from }));
    }
}
