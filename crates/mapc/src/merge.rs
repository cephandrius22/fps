//! Merging fragments back together. CSG and plane cuts leave a face in
//! pieces; neighbors on one plane with one texture mapping rejoin when
//! the result stays convex:
//!
//! ```text
//!   ┌────┬────┐        ┌─────────┐
//!   │ A  │ B  │  ──►   │  A + B  │   shared edge removed, corners
//!   └────┴────┘        └─────────┘   left in a straight line dropped
//! ```
//!
//! Fewer, larger faces: fewer triangles and lightmaps. Gaps where one
//! face's edge meets the middle of another's (T-junctions) are not
//! fixed.

use glam::DVec3;

use crate::csg::Fragment;
use crate::projection::Projection;
use crate::solid::Solid;
use crate::winding::Winding;

/// Corners this close are one corner, map units.
const POINT_EPSILON: f64 = 0.01;
/// Turns this sharp (sine, by length) or gentler count as straight.
const COLLINEAR_EPSILON: f64 = 1e-6;

/// `fragments` with mergeable neighbors joined.
pub(crate) fn merge(fragments: Vec<Fragment>, solids: &[Solid]) -> Vec<Fragment> {
    let mut groups: Vec<Vec<Fragment>> = Vec::new();
    for fragment in fragments {
        let group = groups
            .iter_mut()
            .find(|g| mergeable(&g[0], &fragment, solids));
        match group {
            Some(group) => group.push(fragment),
            None => groups.push(vec![fragment]),
        }
    }

    groups
        .into_iter()
        .flat_map(|group| merge_group(group, solids))
        .collect()
}

/// Same plane, same texture and mapping, same contents.
fn mergeable(a: &Fragment, b: &Fragment, solids: &[Solid]) -> bool {
    let (sa, sb) = (&solids[a.solid], &solids[b.solid]);
    let (side_a, side_b) = (&sa.sides[a.side], &sb.sides[b.side]);

    sa.contents == sb.contents
        && side_a.plane.same(&side_b.plane)
        && side_a.face.texture == side_b.face.texture
        && Projection::new(side_a.face, side_a.plane.normal)
            .matches(&Projection::new(side_b.face, side_b.plane.normal))
}

/// Joins pairs within one group until none join.
fn merge_group(mut group: Vec<Fragment>, solids: &[Solid]) -> Vec<Fragment> {
    let normal = solids[group[0].solid].sides[group[0].side].plane.normal;

    'search: loop {
        for i in 0..group.len() {
            for j in i + 1..group.len() {
                let Some(joined) = join(&group[i].winding, &group[j].winding, normal) else {
                    continue;
                };
                group[i].winding = joined;
                group.swap_remove(j);
                continue 'search;
            }
        }
        return group;
    }
}

/// `a` and `b` joined along an edge they share in opposite directions,
/// if the result is convex.
fn join(a: &Winding, b: &Winding, normal: DVec3) -> Option<Winding> {
    let (pa, pb) = (a.points(), b.points());
    let (na, nb) = (pa.len(), pb.len());
    let same = |p: DVec3, q: DVec3| p.abs_diff_eq(q, POINT_EPSILON);

    // a[i] → a[i+1] is b[j+1] → b[j].
    let (i, j) = (0..na).find_map(|i| {
        let (a0, a1) = (pa[i], pa[(i + 1) % na]);
        (0..nb)
            .find(|&j| same(pb[j], a1) && same(pb[(j + 1) % nb], a0))
            .map(|j| (i, j))
    })?;

    // All of a from a[i+1] round to a[i], then b past the shared edge.
    let from_a = (1..=na).map(|k| pa[(i + k) % na]);
    let from_b = (2..nb).map(|k| pb[(j + k) % nb]);
    let points: Vec<DVec3> = from_a.chain(from_b).collect();

    let points = without_collinear(&points, normal);
    convex(&points, normal)
        .then(|| Winding::new(points))
        .flatten()
}

/// Whether every corner turns the same way (counter-clockwise about
/// `normal`), straight runs allowed.
fn convex(points: &[DVec3], normal: DVec3) -> bool {
    turns(points, normal).all(|turn| turn >= -COLLINEAR_EPSILON)
}

/// Corners that are not a straight continuation of their edges.
fn without_collinear(points: &[DVec3], normal: DVec3) -> Vec<DVec3> {
    let keep: Vec<bool> = turns(points, normal)
        .map(|turn| turn.abs() > COLLINEAR_EPSILON)
        .collect();
    points
        .iter()
        .zip(keep)
        .filter_map(|(&p, keep)| keep.then_some(p))
        .collect()
}

/// Per corner, the sine of its turn (positive counter-clockwise about
/// `normal`).
fn turns(points: &[DVec3], normal: DVec3) -> impl Iterator<Item = f64> + '_ {
    let n = points.len();
    (0..n).map(move |k| {
        let prev = points[(k + n - 1) % n];
        let (here, next) = (points[k], points[(k + 1) % n]);
        let (into, out) = (
            (here - prev).normalize_or_zero(),
            (next - here).normalize_or_zero(),
        );
        into.cross(out).dot(normal)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csg;
    use crate::testing::cuboid;

    fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Winding {
        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        Winding::new(corners.map(|(x, y)| DVec3::new(x, y, 0.0)).to_vec()).unwrap()
    }

    #[test]
    fn neighbors_join_into_one_without_the_shared_edge() {
        let joined = join(
            &square(0.0, 0.0, 32.0, 64.0),
            &square(32.0, 0.0, 64.0, 64.0),
            DVec3::Z,
        );
        let joined = joined.unwrap();

        assert_eq!(joined.points().len(), 4, "the midpoints were straight");
        assert_eq!(joined.area(), 64.0 * 64.0);
        assert!(joined.newell().z > 0.0);
    }

    #[test]
    fn concave_or_apart_pieces_stay_apart() {
        // An L: the join would not be convex.
        let tall = square(0.0, 0.0, 32.0, 64.0);
        let short = square(32.0, 0.0, 64.0, 32.0);
        assert!(join(&tall, &short, DVec3::Z).is_none(), "L shape");

        let apart = square(40.0, 0.0, 64.0, 64.0);
        assert!(join(&tall, &apart, DVec3::Z).is_none(), "no shared edge");
    }

    #[test]
    fn coplanar_brush_tops_merge_but_textures_do_not_mix() {
        // Four brushes in a row: the first three share a texture.
        let brushes = [
            cuboid(DVec3::ZERO, DVec3::new(64.0, 64.0, 16.0), "floor"),
            cuboid(
                DVec3::new(64.0, 0.0, 0.0),
                DVec3::new(128.0, 64.0, 16.0),
                "floor",
            ),
            cuboid(
                DVec3::new(128.0, 0.0, 0.0),
                DVec3::new(192.0, 64.0, 16.0),
                "floor",
            ),
            cuboid(
                DVec3::new(192.0, 0.0, 0.0),
                DVec3::new(256.0, 64.0, 16.0),
                "crate",
            ),
        ];
        let solids: Vec<Solid> = brushes.iter().map(|b| Solid::build(b).unwrap()).collect();
        let merged = merge(csg::visible(&solids), &solids);

        let tops: Vec<&Fragment> = merged
            .iter()
            .filter(|f| solids[f.solid].sides[f.side].plane.normal == DVec3::Z)
            .collect();
        assert_eq!(tops.len(), 2, "floor and crate");
        let areas: Vec<f64> = tops.iter().map(|f| f.winding.area()).collect();
        assert!(areas.contains(&(192.0 * 64.0)) && areas.contains(&(64.0 * 64.0)));
    }
}
