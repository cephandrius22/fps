//! The committed test map end to end: maps/test.map compiled by mapc
//! and lit by bake, checked against what the source says, and against
//! the committed assets/maps/test.lvl.

use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use level::{Face, Level, LuxelGrid};

/// The world's brushes in the source; models 1..=5 have one each.
const WORLD_BRUSHES: usize = 23;
/// Map units in front of a face where open space must be.
const IN_FRONT: f32 = 1.0;

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
    bake::bake(
        &mut level,
        &mapc::png_albedos(&root().join("assets/textures")),
    );
    level
}

fn centroid(face: &Face) -> Vec3 {
    face.points.iter().sum::<Vec3>() / face.points.len() as f32
}

#[test]
fn entities_and_models_follow_the_source() {
    let level = compiled();

    let keys: Vec<(&str, Option<usize>)> = level
        .entities
        .iter()
        .map(|e| (e.classname(), e.model()))
        .collect();
    let mut expected = vec![
        ("worldspawn", Some(0)),
        ("info_player_start", None),
        ("func_door", Some(1)),
        ("func_button", Some(2)),
        ("trigger_multiple", Some(3)),
        ("trigger_once", Some(4)),
        ("func_door", Some(5)),
    ];
    expected.extend([("light", None); 5]);
    expected.extend([("prop_static", None); 2]);
    assert_eq!(keys, expected);

    let brushes: Vec<usize> = level.models.iter().map(|m| m.brushes.len()).collect();
    assert_eq!(brushes, [WORLD_BRUSHES, 1, 1, 1, 1, 1]);
    assert!(
        level.models[3].faces.is_empty(),
        "trigger brushes draw nothing"
    );
}

#[test]
fn props_get_a_baked_light_level() {
    let level = lit();
    let props = level
        .entities
        .iter()
        .filter(|e| e.classname() == "prop_static");

    for prop in props {
        let light: u8 = prop.get(entities::BAKED_LIGHT).unwrap().parse().unwrap();
        assert!(light > 0, "a lit room");
    }
}

#[test]
fn textures_are_sized_from_their_pngs() {
    let level = compiled();
    let names: Vec<&str> = level.textures.iter().map(|t| t.name.as_str()).collect();

    assert_eq!(names, ["floor", "crate"]);
    assert!(level.textures.iter().all(|t| t.width > 0 && t.height > 0));
}

#[test]
fn the_world_is_sealed_and_shows_no_outside() {
    let level = compiled();
    let world = &level.models[0];

    // Outside faces lie on the world's bounds, facing out of them.
    for face in &world.faces {
        let ahead = centroid(face) + face.normal * IN_FRONT;
        let inside = ahead.cmpgt(world.mins).all() && ahead.cmplt(world.maxs).all();
        assert!(inside, "face at {} faces the void", centroid(face));
    }
}

#[test]
fn floors_map_textures_as_quake_does() {
    let level = compiled();
    let floor = level.models[0]
        .faces
        .iter()
        .find(|f| f.normal == Vec3::Z && f.points[0].z == 0.0)
        .unwrap();

    for (p, t) in floor.points.iter().zip(&floor.texels) {
        assert_eq!(*t, Vec2::new(p.x, -p.y));
    }
}

#[test]
fn every_drawn_face_is_lit_on_its_own_grid() {
    let level = lit();

    for face in level.models.iter().flat_map(|m| &m.faces) {
        let lightmap = face.lightmap.as_ref().expect("lit");
        let grid = LuxelGrid::new(&face.texels);
        assert_eq!([lightmap.width, lightmap.height], grid.size);
        assert_eq!(lightmap.coords, grid.coords(&face.texels));
    }

    // Some light reaches every room's floor.
    let floors = level.models[0].faces.iter().filter(|f| f.normal == Vec3::Z);
    let dark = floors.filter(|f| f.lightmap.as_ref().unwrap().luxels.iter().all(|&l| l == 0));
    assert_eq!(dark.count(), 0);
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
