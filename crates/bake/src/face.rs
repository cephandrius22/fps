//! Lighting one face: a sample point per luxel, lit by every light it
//! can see.
//!
//! ```text
//!   luxel (x, y) ─► texel (s, t) ─► point on the face's plane
//!                ─► pulled inside the face, if it fell outside
//!                ─► lifted off the surface ─► Σ visible lights
//! ```
//!
//! Luxels on the grid's edge often fall just outside the polygon, where
//! a neighboring wall would shadow them; pulling them onto the face
//! keeps edges as bright as their face.

use glam::{DMat2, DVec2, DVec3};
use level::{Face, Lightmap, LuxelGrid};

use crate::lights::{Light, Settings};
use crate::occlusion::Occluders;

/// Samples sit this far off the surface, map units, so the face's own
/// brush never shadows them.
const SAMPLE_OFFSET: f64 = 1.0;
/// Quake's light tool halves light before storing it: a luxel of 128 is
/// a texture at full brightness, leaving room to overbright.
const RANGE_SCALE: f64 = 0.5;
const MAX_LUXEL: f64 = u8::MAX as f64;
/// Texel axes closer to parallel than this cannot map back to points.
const MIN_DETERMINANT: f64 = 1e-9;

/// What lights a face.
pub(crate) struct Scene<'a> {
    pub lights: &'a [Light],
    pub occluders: &'a Occluders,
    pub settings: &'a Settings,
}

/// The face's lightmap; None when its texels do not span its plane
/// (a degenerate mapping), which leaves it unlit.
pub(crate) fn light_face(face: &Face, scene: &Scene) -> Option<Lightmap> {
    let to_point = TexelMap::new(face)?;
    let grid = LuxelGrid::new(&face.texels);
    let normal = face.normal.as_dvec3();
    let polygon: Vec<DVec3> = face.points.iter().map(|p| p.as_dvec3()).collect();

    let [width, height] = grid.size;
    let luxels = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| {
            let point = to_point.point(grid.texel(x, y));
            let sample = inside(&polygon, normal, point) + normal * SAMPLE_OFFSET;
            luxel(light_at(sample, normal, scene))
        })
        .collect();

    Some(Lightmap {
        width,
        height,
        luxels,
        coords: grid.coords(&face.texels),
    })
}

/// Total light at `point` on a surface facing `normal`.
fn light_at(point: DVec3, normal: DVec3, scene: &Scene) -> f64 {
    let lit: f64 = scene
        .lights
        .iter()
        .map(|light| (light, light.at(point, normal, scene.settings)))
        .filter(|&(light, amount)| amount > 0.0 && !scene.occluders.blocked(point, light.origin))
        .map(|(_, amount)| amount)
        .sum();
    lit.max(scene.settings.minlight)
}

/// Light as a stored luxel.
fn luxel(light: f64) -> u8 {
    (light * RANGE_SCALE).round().clamp(0.0, MAX_LUXEL) as u8
}

/// Texels back to points on the face's plane: the inverse of its
/// texture mapping, solved from three corners.
#[derive(Debug)]
struct TexelMap {
    /// A corner, and its texel.
    origin: DVec3,
    origin_texel: DVec2,
    /// Map units per texel along s and t.
    along: [DVec3; 2],
}

impl TexelMap {
    /// Uses the corner fan triangle whose texels span the most area,
    /// for precision. None when every one is degenerate.
    fn new(face: &Face) -> Option<Self> {
        let points: Vec<DVec3> = face.points.iter().map(|p| p.as_dvec3()).collect();
        let texels: Vec<DVec2> = face.texels.iter().map(|t| t.as_dvec2()).collect();

        let spans = (1..points.len().saturating_sub(1)).map(|i| {
            let edges = DMat2::from_cols(texels[i] - texels[0], texels[i + 1] - texels[0]);
            (i, edges)
        });
        let (i, edges) =
            spans.max_by(|a, b| a.1.determinant().abs().total_cmp(&b.1.determinant().abs()))?;
        if edges.determinant().abs() < MIN_DETERMINANT {
            return None;
        }

        // A texel offset d maps to corner weights edges⁻¹·d, and those
        // weights to a point offset along the two corner edges.
        let inverse = edges.inverse();
        let (u, v) = (points[i] - points[0], points[i + 1] - points[0]);
        let along = [inverse.col(0), inverse.col(1)].map(|w| u * w.x + v * w.y);
        Some(Self {
            origin: points[0],
            origin_texel: texels[0],
            along,
        })
    }

    fn point(&self, texel: DVec2) -> DVec3 {
        let d = texel - self.origin_texel;
        self.origin + self.along[0] * d.x + self.along[1] * d.y
    }
}

/// `point` (on the polygon's plane), or the nearest point of the
/// polygon's edges when it lies outside. Corners run counter-clockwise
/// seen along `normal`.
fn inside(polygon: &[DVec3], normal: DVec3, point: DVec3) -> DVec3 {
    let edges = || polygon.iter().zip(polygon.iter().cycle().skip(1));
    let within = edges().all(|(&a, &b)| (b - a).cross(point - a).dot(normal) >= 0.0);
    if within {
        return point;
    }

    edges()
        .map(|(&a, &b)| nearest_on_segment(a, b, point))
        .min_by(|p, q| {
            p.distance_squared(point)
                .total_cmp(&q.distance_squared(point))
        })
        .unwrap_or(point)
}

fn nearest_on_segment(a: DVec3, b: DVec3, p: DVec3) -> DVec3 {
    let ab = b - a;
    let t = (p - a).dot(ab) / ab.length_squared().max(f64::MIN_POSITIVE);
    a + ab * t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lights::Falloff;
    use crate::testing::{cuboid_model, floor, level_with};
    use glam::Vec2;

    const EPS: f64 = 1e-9;

    fn settings() -> Settings {
        Settings {
            minlight: 0.0,
            angle_scale: 0.5,
        }
    }

    fn lamp(origin: DVec3) -> Light {
        Light {
            origin,
            brightness: 300.0,
            scale: 1.0,
            falloff: Falloff::Linear,
        }
    }

    /// Luxel (x, y) of a lightmap.
    fn at(lightmap: &Lightmap, x: u32, y: u32) -> u8 {
        lightmap.luxels[(y * lightmap.width + x) as usize]
    }

    #[test]
    fn texel_map_inverts_the_face_mapping() {
        let mut face = floor(64.0, 8.0);
        // Rotated and scaled texels: s = 2y + 5, t = x / 2.
        face.texels = face
            .points
            .iter()
            .map(|p| Vec2::new(2.0 * p.y + 5.0, p.x / 2.0))
            .collect();
        let map = TexelMap::new(&face).unwrap();

        let point = map.point(DVec2::new(2.0 * 10.0 + 5.0, 30.0 / 2.0));
        assert!(point.abs_diff_eq(DVec3::new(30.0, 10.0, 8.0), EPS));

        face.texels = vec![Vec2::ONE; 4];
        assert!(TexelMap::new(&face).is_none(), "no span");
    }

    #[test]
    fn outside_points_are_pulled_onto_the_polygon() {
        let face = floor(64.0, 0.0);
        let polygon: Vec<DVec3> = face.points.iter().map(|p| p.as_dvec3()).collect();

        let within = DVec3::new(10.0, 20.0, 0.0);
        assert_eq!(inside(&polygon, DVec3::Z, within), within);
        let beyond = DVec3::new(70.0, 20.0, 0.0);
        assert_eq!(
            inside(&polygon, DVec3::Z, beyond),
            DVec3::new(64.0, 20.0, 0.0)
        );
        let corner = DVec3::new(-5.0, -5.0, 0.0);
        assert_eq!(inside(&polygon, DVec3::Z, corner), DVec3::ZERO);
    }

    #[test]
    fn a_floor_is_brightest_under_its_light() {
        let level = level_with(Vec::new());
        let occluders = Occluders::new(&level);
        let lights = [lamp(DVec3::new(0.0, 0.0, 100.0))];
        let scene = Scene {
            lights: &lights,
            occluders: &occluders,
            settings: &settings(),
        };
        let lightmap = light_face(&floor(64.0, 0.0), &scene).unwrap();

        // 64 units: luxels 0..4 each way; texel t = −y, so row 0 is y 0.
        assert_eq!((lightmap.width, lightmap.height), (5, 5));
        assert_eq!(lightmap.coords[0], Vec2::new(0.5, 4.5));
        // Straight below, one unit up: (300 − 99) × 0.5.
        assert_eq!(at(&lightmap, 0, 4), 101);
        assert!(at(&lightmap, 4, 0) < at(&lightmap, 0, 4), "fades away");
    }

    #[test]
    fn a_box_casts_a_shadow_and_minlight_fills_it() {
        // A box hanging between the light and the floor's far half.
        let blocker = cuboid_model(DVec3::new(32.0, -8.0, 40.0), DVec3::new(80.0, 80.0, 48.0));
        let occluders = Occluders::new(&level_with(vec![blocker]));
        let lights = [lamp(DVec3::new(48.0, 32.0, 100.0))];
        let bake = |settings: &Settings| {
            let scene = Scene {
                lights: &lights,
                occluders: &occluders,
                settings,
            };
            light_face(&floor(64.0, 0.0), &scene).unwrap()
        };

        let lightmap = bake(&settings());
        assert_eq!(at(&lightmap, 4, 2), 0, "under the box");
        assert!(at(&lightmap, 0, 2) > 50, "beside it");

        let dim = Settings {
            minlight: 40.0,
            ..settings()
        };
        assert_eq!(at(&bake(&dim), 4, 2), 20);
    }

    #[test]
    fn light_saturates_at_the_brightest_luxel() {
        assert_eq!(luxel(1000.0), u8::MAX);
        assert_eq!(luxel(-5.0), 0);
        assert_eq!(luxel(64.0), 32);
    }
}
