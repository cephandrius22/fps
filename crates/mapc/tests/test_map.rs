//! mapc and bake against ericw-tools: compiles and lights
//! maps/test.map and checks the result against assets/maps/test.bsp,
//! which qbsp and light built from the same source. They split faces
//! differently (qbsp along its BSP tree), so faces are compared by
//! coverage, texture mapping and light at sample points, not one to
//! one.

use std::path::{Path, PathBuf};

use glam::{DVec2, Vec3};
use level::{Face, Level};

/// Plane and position agreement, map units.
const EPS: f64 = 0.01;
/// Texel agreement: f32 storage of large coordinates.
const TEXEL_EPS: f64 = 0.01;
/// Normals this aligned are one direction.
const SAME_DIRECTION: f32 = 0.999;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compiled() -> Level {
    let text = std::fs::read_to_string(root().join("maps/test.map")).unwrap();
    let map = mapfile::parse(&text).unwrap();
    mapc::compile(&map, &mapc::png_sizes(&root().join("assets/textures")))
}

fn lit() -> Level {
    let mut level = compiled();
    bake::bake(&mut level);
    level
}

fn qbsp() -> Level {
    bsp::parse(&std::fs::read(root().join("assets/maps/test.bsp")).unwrap()).unwrap()
}

fn centroid(face: &Face) -> Vec3 {
    face.points.iter().sum::<Vec3>() / face.points.len() as f32
}

/// Whether `p` lies on the face's plane, within the face.
fn contains(face: &Face, p: Vec3) -> bool {
    let p = p.as_dvec3();
    let normal = face.normal.as_dvec3();
    if normal.dot(p - face.points[0].as_dvec3()).abs() > EPS {
        return false;
    }

    let next = face.points.iter().cycle().skip(1);
    face.points.iter().zip(next).all(|(a, b)| {
        let (a, b) = (a.as_dvec3(), b.as_dvec3());
        (b - a).cross(p - a).dot(normal) >= -EPS
    })
}

/// The face's texel at `p`, by its affine mapping through the corner
/// fan triangle spanning the most area (qbsp keeps collinear corners).
fn texel_at(face: &Face, p: Vec3) -> DVec2 {
    let corners = 1..face.points.len() - 1;
    let area = |i: usize| {
        let [a, b, c] = [0, i, i + 1].map(|k| face.points[k].as_dvec3());
        (b - a).cross(c - a).length()
    };
    let i = corners
        .max_by(|&x, &y| area(x).total_cmp(&area(y)))
        .unwrap();

    let [a, b, c] = [0, i, i + 1].map(|k| face.points[k].as_dvec3());
    let [ta, tb, tc] = [0, i, i + 1].map(|k| face.texels[k].as_dvec2());
    let (u, v) = (b - a, c - a);
    let w = p.as_dvec3() - a;

    // Solve w = s·u + t·v in the plane (normal equations).
    let (uu, uv, vv) = (u.dot(u), u.dot(v), v.dot(v));
    let (wu, wv) = (w.dot(u), w.dot(v));
    let det = uu * vv - uv * uv;
    let s = (wu * vv - wv * uv) / det;
    let t = (wv * uu - wu * uv) / det;
    ta + (tb - ta) * s + (tc - ta) * t
}

#[test]
fn entities_and_models_match_qbsp() {
    let (ours, theirs) = (compiled(), qbsp());

    let keys = |level: &Level| -> Vec<(String, Option<usize>)> {
        let entities = level.entities.iter();
        entities
            .map(|e| (e.classname().to_owned(), e.model()))
            .collect()
    };
    assert_eq!(keys(&ours), keys(&theirs));
    assert_eq!(ours.models.len(), theirs.models.len());
}

#[test]
fn brushes_have_qbsp_planes_and_bounds() {
    let (ours, theirs) = (compiled(), qbsp());

    for (index, (a, b)) in ours.models.iter().zip(&theirs.models).enumerate() {
        assert_eq!(a.brushes.len(), b.brushes.len(), "model {index}");

        for (x, y) in a.brushes.iter().zip(&b.brushes) {
            assert_eq!(x.contents, y.contents);
            assert!(
                x.mins.abs_diff_eq(y.mins, EPS as f32),
                "{} vs {}",
                x.mins,
                y.mins
            );
            assert!(
                x.maxs.abs_diff_eq(y.maxs, EPS as f32),
                "{} vs {}",
                x.maxs,
                y.maxs
            );

            // qbsp may add bevel planes; every one of ours is among its.
            for plane in &x.planes {
                let found = y.planes.iter().any(|p| {
                    p.normal.abs_diff_eq(plane.normal, 1e-5)
                        && (f64::from(p.dist) - f64::from(plane.dist)).abs() < EPS
                });
                assert!(found, "model {index}: plane {plane:?}");
            }
        }
    }
}

#[test]
fn every_qbsp_face_is_covered_with_the_same_texels() {
    let (ours, theirs) = (compiled(), qbsp());
    let name = |level: &Level, face: &Face| level.textures[face.texture].name.clone();
    let mut checked = 0;

    for (index, (a, b)) in ours.models.iter().zip(&theirs.models).enumerate() {
        for face in &b.faces {
            let texture = name(&theirs, face);
            if level::is_tool_texture(&texture) {
                continue;
            }

            let center = centroid(face);
            let ours = a.faces.iter().find(|f| {
                name(&ours, f) == texture
                    && f.normal.dot(face.normal) > SAME_DIRECTION
                    && contains(f, center)
            });
            let Some(ours) = ours else {
                panic!("model {index}: no face covers {texture} at {center}");
            };

            let expected =
                face.texels.iter().map(|t| t.as_dvec2()).sum::<DVec2>() / face.texels.len() as f64;
            let got = texel_at(ours, center);
            assert!(
                got.abs_diff_eq(expected, TEXEL_EPS),
                "model {index}: {texture} at {center}: {got} vs {expected}"
            );
            checked += 1;
        }
    }

    let total: usize = theirs.models.iter().map(|m| m.faces.len()).sum();
    eprintln!("checked {checked} of {total} qbsp faces");
    assert!(checked > total / 2, "most faces are drawn, not tools");
}

#[test]
fn texture_sizes_match_qbsp() {
    let (ours, theirs) = (compiled(), qbsp());

    for texture in &ours.textures {
        let theirs = theirs
            .textures
            .iter()
            .find(|t| t.name == texture.name)
            .unwrap();
        assert_eq!(
            (texture.width, texture.height),
            (theirs.width, theirs.height)
        );
    }
}

/// The face's light at `p`, bilinear between luxel centers as the
/// engine samples it.
fn light_at(face: &Face, p: Vec3) -> f64 {
    let lightmap = face.lightmap.as_ref().expect("lit");
    let grid = level::LuxelGrid::new(&face.texels);
    let coord = grid.coords(&[texel_at(face, p).as_vec2()])[0].as_dvec2() - 0.5;

    let max = DVec2::new(
        f64::from(lightmap.width - 1),
        f64::from(lightmap.height - 1),
    );
    let coord = coord.clamp(DVec2::ZERO, max);
    let (lo, frac) = (coord.floor(), coord - coord.floor());
    let luxel = |x: f64, y: f64| {
        let (x, y) = (x.min(max.x) as u32, y.min(max.y) as u32);
        f64::from(lightmap.luxels[(y * lightmap.width + x) as usize])
    };

    let top = luxel(lo.x, lo.y) * (1.0 - frac.x) + luxel(lo.x + 1.0, lo.y) * frac.x;
    let bottom = luxel(lo.x, lo.y + 1.0) * (1.0 - frac.x) + luxel(lo.x + 1.0, lo.y + 1.0) * frac.x;
    top * (1.0 - frac.y) + bottom * frac.y
}

/// Light agreement on world faces, luxel levels of 255: rounding, and
/// ericw placing edge samples its own way.
const LIGHT_EPS: f64 = 2.0;

/// World faces only: ericw gives every face of a brush entity (a door)
/// one flat value, while bake lights each from where it sits.
#[test]
fn world_light_matches_ericw_light() {
    let (ours, theirs) = (lit(), qbsp());
    let (world, qbsp_world) = (&ours.models[0], &theirs.models[0]);
    let mut checked = 0;

    for face in qbsp_world.faces.iter().filter(|f| f.lightmap.is_some()) {
        let center = centroid(face);
        let ours = world
            .faces
            .iter()
            .find(|f| f.normal.dot(face.normal) > SAME_DIRECTION && contains(f, center))
            .expect("coverage is checked above");

        let (got, expected) = (light_at(ours, center), light_at(face, center));
        assert!(
            (got - expected).abs() <= LIGHT_EPS,
            "light at {center}: {got:.1} vs {expected:.1}"
        );
        checked += 1;
    }

    assert!(checked > qbsp_world.faces.len() / 2);
}

/// The committed .lvl is what mapc builds today, so the engine's tests
/// and the game never run a stale compile. Rebuild it with
/// `cargo run -p mapc -- maps/test.map assets/maps/test.lvl`.
#[test]
fn committed_level_is_current() {
    let committed = std::fs::read(root().join("assets/maps/test.lvl")).unwrap();

    assert!(
        level::write(&lit()) == committed,
        "assets/maps/test.lvl is stale: recompile it with mapc"
    );
}
