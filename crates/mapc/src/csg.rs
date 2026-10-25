//! Hidden-surface removal between the brushes of one model: the parts
//! of each face buried inside another brush are cut away, so overlaps
//! and shared walls draw nothing.
//!
//! ```text
//!   ┌──────┐               ┌──────┐
//!   │  A ┌─┼────┐          │  A   └────┐
//!   │    │ │ B  │   ──►    │      ┌────┘ B
//!   └────┼─┘    │          └──────┤
//!        └──────┘                 └ (faces inside the other are gone)
//! ```
//!
//! Each face is cut by every plane of each other brush in turn: pieces
//! in front of a plane are outside that brush and survive; what is left
//! behind all of them is inside and dropped. A face lying on the other
//! brush's surface counts as inside when they face each other (a shared
//! wall, which nobody can see) and, when they face the same way, inside
//! only for the later brush, so exactly one copy is kept.
//!
//! Not done: merging the pieces back together, or removing faces that
//! face the void outside a sealed map.

use glam::DVec3;
use level::Contents;

use crate::solid::Solid;
use crate::winding::{Cut, Winding};

/// Bounds this close count as touching, map units.
const TOUCH_EPSILON: f64 = 0.1;
/// Pieces smaller than this are slivers from rounding, square units.
const MIN_AREA: f64 = 0.01;

/// A visible piece of one side of one solid.
#[derive(Debug)]
pub(crate) struct Fragment {
    /// Index of its solid.
    pub solid: usize,
    /// Index of the side within the solid.
    pub side: usize,
    /// The visible polygon, wound like the side's.
    pub winding: Winding,
}

/// The visible pieces of every side of `solids`.
pub(crate) fn visible(solids: &[Solid]) -> Vec<Fragment> {
    let mut fragments = Vec::new();

    for (i, owner) in solids.iter().enumerate() {
        for (s, side) in owner.sides.iter().enumerate() {
            let mut pieces = vec![side.winding.clone()];
            for (j, other) in solids.iter().enumerate() {
                if j == i || !hides(other.contents, owner.contents) || !touches(owner, other) {
                    continue;
                }
                let order = match j < i {
                    true => Order::Later,
                    false => Order::Earlier,
                };
                pieces = pieces
                    .into_iter()
                    .flat_map(|piece| outside(piece, side.plane.normal, other, order))
                    .collect();
            }

            let pieces = pieces.into_iter().filter(|w| w.area() >= MIN_AREA);
            fragments.extend(pieces.map(|winding| Fragment {
                solid: i,
                side: s,
                winding,
            }));
        }
    }

    fragments
}

/// The face owner's place in the brush list relative to the brush
/// cutting it.
#[derive(Clone, Copy, PartialEq)]
enum Order {
    Earlier,
    Later,
}

/// Whether brushes of `other` contents hide faces of `owner` contents
/// inside them. Solids and sky hide everything; liquids and clip hide
/// only their own kind (no walls between adjacent water brushes, but
/// walls stay visible under water).
fn hides(other: Contents, owner: Contents) -> bool {
    matches!(other, Contents::Solid | Contents::Sky) || other == owner
}

/// Whether two solids' bounds overlap or touch.
fn touches(a: &Solid, b: &Solid) -> bool {
    let apart = a.mins.cmpgt(b.maxs + TOUCH_EPSILON) | b.mins.cmpgt(a.maxs + TOUCH_EPSILON);
    !apart.any()
}

/// The pieces of `winding` (facing `normal`) outside `other`.
fn outside(winding: Winding, normal: DVec3, other: &Solid, order: Order) -> Vec<Winding> {
    let mut outside = Vec::new();
    let mut inside = winding;

    for side in &other.sides {
        match inside.cut(&side.plane) {
            Cut::Front => {
                outside.push(inside);
                return outside;
            }
            Cut::Back => {}
            Cut::Split { front, back } => {
                outside.push(front);
                inside = back;
            }
            // Same facing: the earlier brush's copy survives. Facing
            // each other: a shared wall, buried.
            Cut::On => {
                let same = normal.dot(side.plane.normal) > 0.0;
                if same && order == Order::Earlier {
                    outside.push(inside);
                    return outside;
                }
            }
        }
    }

    // Behind every plane: inside `other`.
    outside
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::cuboid;

    fn solids(brushes: &[mapfile::Brush]) -> Vec<Solid<'_>> {
        brushes.iter().map(|b| Solid::build(b).unwrap()).collect()
    }

    /// Total visible area, square units.
    fn area(fragments: &[Fragment]) -> f64 {
        fragments.iter().map(|f| f.winding.area()).sum()
    }

    /// Total visible area of sides facing `normal`.
    fn facing(solids: &[Solid], fragments: &[Fragment], normal: DVec3) -> f64 {
        let fragments: Vec<_> = fragments
            .iter()
            .filter(|f| solids[f.solid].sides[f.side].plane.normal == normal)
            .collect();
        fragments.iter().map(|f| f.winding.area()).sum()
    }

    #[test]
    fn a_lone_brush_shows_every_side() {
        let brushes = [cuboid(DVec3::ZERO, DVec3::splat(64.0), "floor")];
        let fragments = visible(&solids(&brushes));

        assert_eq!(fragments.len(), 6);
        assert_eq!(area(&fragments), 6.0 * 64.0 * 64.0);
    }

    #[test]
    fn a_shared_wall_vanishes_from_both_sides() {
        let brushes = [
            cuboid(DVec3::ZERO, DVec3::splat(64.0), "floor"),
            cuboid(
                DVec3::new(64.0, 0.0, 0.0),
                DVec3::new(128.0, 64.0, 64.0),
                "floor",
            ),
        ];
        let solids = solids(&brushes);
        let fragments = visible(&solids);

        // The outside of a 128 × 64 × 64 box: no wall at x = 64.
        assert_eq!(area(&fragments), 2.0 * (128.0 * 64.0 * 2.0 + 64.0 * 64.0));
        assert!(
            fragments
                .iter()
                .all(|f| f.winding.points().iter().any(|p| p.x != 64.0))
        );
        assert_eq!(facing(&solids, &fragments, DVec3::Z), 128.0 * 64.0);
    }

    #[test]
    fn overlaps_are_cut_and_coplanar_faces_kept_once() {
        // B overlaps A by half, sharing A's top and bottom planes.
        let brushes = [
            cuboid(DVec3::ZERO, DVec3::splat(64.0), "floor"),
            cuboid(
                DVec3::new(32.0, 0.0, 0.0),
                DVec3::new(96.0, 64.0, 64.0),
                "crate",
            ),
        ];
        let solids = solids(&brushes);
        let fragments = visible(&solids);

        assert_eq!(area(&fragments), 2.0 * (96.0 * 64.0 * 2.0 + 64.0 * 64.0));
        assert_eq!(facing(&solids, &fragments, DVec3::Z), 96.0 * 64.0);

        // The shared stretch of top belongs to A, the earlier brush.
        let b_top = fragments
            .iter()
            .filter(|f| f.solid == 1 && solids[1].sides[f.side].plane.normal == DVec3::Z);
        assert!(b_top.flat_map(|f| f.winding.points()).all(|p| p.x >= 64.0));
    }

    #[test]
    fn liquids_hide_only_liquid() {
        // A wall half sunk in water, beside more water.
        let brushes = [
            cuboid(DVec3::ZERO, DVec3::splat(64.0), "floor"),
            cuboid(
                DVec3::new(-32.0, -32.0, 0.0),
                DVec3::new(32.0, 96.0, 32.0),
                "*water",
            ),
            cuboid(
                DVec3::new(-96.0, -32.0, 0.0),
                DVec3::new(-32.0, 96.0, 32.0),
                "*water",
            ),
        ];
        let solids = solids(&brushes);
        let fragments = visible(&solids);
        let wall: f64 = fragments
            .iter()
            .filter(|f| f.solid == 0)
            .map(|f| f.winding.area())
            .sum();

        assert_eq!(wall, 6.0 * 64.0 * 64.0, "the wall shows through water");
        let between = |f: &&Fragment| f.winding.points().iter().all(|p| p.x == -32.0);
        assert_eq!(
            fragments.iter().filter(between).count(),
            0,
            "no wall between waters"
        );
    }
}
