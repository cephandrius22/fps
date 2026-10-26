//! The committed test prop, assets/models/barrel.glb (built by
//! `cargo run -p props --example barrel -- assets/models/barrel.glb`),
//! reads back whole, with every triangle facing out.

use std::path::Path;

use glam::Vec3;

#[test]
fn barrel_reads_with_outward_triangles() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/barrel.glb");
    let prop = props::read(&std::fs::read(path).unwrap()).unwrap();

    assert_eq!((prop.meshes.len(), prop.textures.len()), (2, 2));
    let (lo, hi) = prop.bounds().unwrap();
    assert!(lo.abs_diff_eq(Vec3::new(-0.35, 0.0, -0.35), 1e-3), "{lo}");
    assert!(hi.abs_diff_eq(Vec3::new(0.35, 0.9, 0.35), 1e-3), "{hi}");

    // Counter-clockwise from outside: each face normal points away from
    // the barrel's middle.
    let middle = (lo + hi) / 2.0;
    for mesh in &prop.meshes {
        for t in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            let normal = (b - a).cross(c - a);
            let center = (a + b + c) / 3.0;
            assert!(
                normal.dot(center - middle) > 0.0,
                "inward triangle at {center}"
            );
        }
    }
}
