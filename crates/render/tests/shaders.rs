//! Validates every shipped WGSL shader on the CPU with naga, the same
//! front end wgpu uses. Runs everywhere, GPU or not.

mod common;

use render::Shader;

const ENTRY_POINTS: [&str; 2] = ["vs_main", "fs_main"];

#[test]
fn shipped_shaders_parse_and_validate() {
    for shader in Shader::ALL {
        let name = common::shader_file(shader);
        let source = common::load_shader(shader).unwrap();

        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|err| panic!("{name}: {}", err.emit_to_string(&source)));

        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|err| panic!("{name}: {err:?}"));

        for entry in ENTRY_POINTS {
            let found = module.entry_points.iter().any(|ep| ep.name == entry);
            assert!(found, "{name}: missing entry point {entry}");
        }
    }
}
