//! Reads the committed test map (maps/test.map, compiled by
//! tools/compile-map.sh) and checks what the engine relies on.

use bsp::{Bsp, Contents, Version};
use glam::Vec3;

const EPS: f32 = 1e-3;

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/maps/test.bsp");
/// Directory entry of the lighting lump: after the version, 8 × 8 bytes.
const LIGHTING_ENTRY: usize = 4 + 8 * 8;
/// ericw-tools aligns each face's lightmap to this many bytes.
const LIGHTMAP_ALIGN: usize = 4;

fn test_map() -> Bsp {
    bsp::parse(&std::fs::read(PATH).unwrap()).unwrap()
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

#[test]
fn lightmaps_tile_the_lighting_lump_exactly() {
    // ericw-tools packs lightmaps back to back, each starting on a
    // 4-byte boundary. If every face's luxel grid matches the one `light`
    // used, their padded sizes sum to the lump exactly.
    let bytes = std::fs::read(PATH).unwrap();
    let entry = &bytes[LIGHTING_ENTRY..LIGHTING_ENTRY + 8];
    let lump_len = u32::from_le_bytes(entry[4..8].try_into().unwrap()) as usize;

    let map = test_map();
    let lightmaps: Vec<_> = map
        .models
        .iter()
        .flat_map(|m| &m.faces)
        .filter_map(|f| f.lightmap.as_ref())
        .collect();
    let total: usize = lightmaps
        .iter()
        .map(|l| l.luxels.len().next_multiple_of(LIGHTMAP_ALIGN))
        .sum();

    assert!(!lightmaps.is_empty());
    assert_eq!(total, lump_len);
}

#[test]
fn lightmap_coords_stay_inside_their_grid() {
    let map = test_map();

    for face in map.models.iter().flat_map(|m| &m.faces) {
        let Some(lightmap) = &face.lightmap else {
            continue;
        };
        assert_eq!(lightmap.coords.len(), face.points.len());
        assert_eq!(
            lightmap.luxels.len(),
            (lightmap.width * lightmap.height) as usize
        );

        // Corners fall between the first and last luxel centers.
        let max = glam::Vec2::new(lightmap.width as f32, lightmap.height as f32) - 0.5;
        for c in &lightmap.coords {
            assert!(c.x >= 0.5 - EPS && c.y >= 0.5 - EPS, "{c:?}");
            assert!(c.x <= max.x + EPS && c.y <= max.y + EPS, "{c:?} of {max:?}");
        }
    }
}

#[test]
fn lit_rooms_are_bright_somewhere() {
    let brightest = test_map()
        .models
        .iter()
        .flat_map(|m| &m.faces)
        .filter_map(|f| f.lightmap.as_ref())
        .flat_map(|l| l.luxels.iter().copied())
        .max()
        .unwrap();

    assert!(brightest > 100, "brightest luxel {brightest}");
}
