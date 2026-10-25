//! Reads the committed test map (maps/test.map, compiled by
//! tools/compile-map.sh) and checks what the engine relies on.

use bsp::{Bsp, Contents, Version};
use glam::Vec3;

const EPS: f32 = 1e-3;

fn test_map() -> Bsp {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps/test.bsp");
    bsp::parse(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn reads_version_textures_and_models() {
    let map = test_map();

    assert_eq!(map.version, Version::Bsp29);
    let names: Vec<_> = map.textures.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"floor") && names.contains(&"crate"),
        "{names:?}"
    );

    let floor = map.textures.iter().find(|t| t.name == "floor").unwrap();
    assert_eq!((floor.width, floor.height), (64, 64));
    // World, two doors, a button, two triggers.
    assert_eq!(map.models.len(), 6);
}

#[test]
fn entities_name_their_models() {
    let map = test_map();
    let classes: Vec<_> = map.entities.iter().map(|e| e.classname()).collect();

    assert_eq!(classes[0], "worldspawn");
    for class in [
        "info_player_start",
        "func_door",
        "func_button",
        "trigger_once",
        "light",
    ] {
        assert!(classes.contains(&class), "missing {class}");
    }

    let door = map
        .entities
        .iter()
        .find(|e| e.get("targetname") == Some("door1"))
        .unwrap();
    assert_eq!(door.model(), Some(1));
    assert_eq!(door.get("angle"), Some("-1"));
}

#[test]
fn faces_wind_counter_clockwise_around_their_normals() {
    let map = test_map();

    for face in map.models.iter().flat_map(|m| &m.faces) {
        assert!(face.points.len() >= 3);
        assert_eq!(face.points.len(), face.texels.len());
        assert!(face.texture < map.textures.len());

        // Summed over all corners: Quake faces often have collinear
        // corners (T-junction fixes), so any three may be in a line.
        let winding: Vec3 = (0..face.points.len())
            .map(|i| {
                let next = face.points[(i + 1) % face.points.len()];
                face.points[i].cross(next)
            })
            .sum();
        assert!(winding.dot(face.normal) > 0.0, "{face:?}");
    }
}

#[test]
fn world_floor_faces_up_at_height_zero() {
    let world = &test_map().models[0];
    let floor = world
        .faces
        .iter()
        .find(|f| f.normal.abs_diff_eq(Vec3::Z, EPS) && f.points.iter().all(|p| p.z.abs() < EPS));

    assert!(floor.is_some(), "no upward face at z = 0");
}

#[test]
fn brush_list_covers_world_and_entities() {
    let map = test_map();
    let world = &map.models[0];

    assert!(
        world.brushes.len() >= 20,
        "{} world brushes",
        world.brushes.len()
    );
    assert!(world.brushes.iter().all(|b| b.contents == Contents::Solid));
    // Each door and the button is one brush; triggers too.
    for model in &map.models[1..] {
        assert_eq!(model.brushes.len(), 1);
    }

    // Axial brushes: just the six bound planes, matching the bounds.
    let brush = &world.brushes[0];
    assert_eq!(brush.planes.len(), 6);
    assert_eq!(brush.planes[0].dist, brush.maxs.x);
    assert_eq!(brush.planes[1].dist, -brush.mins.x);
}
