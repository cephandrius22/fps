//! The committed TrenchBroom files: the FGD is what the catalog
//! generates (regenerate it with
//! `cargo run -p entities -- trenchbroom/RetroFPS.fgd`), and the game
//! configuration points at it and at textures that exist.

use std::path::{Path, PathBuf};

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// Tool textures the game configuration tags.
const TOOL_TEXTURES: [&str; 4] = ["trigger", "clip", "skip", "hint"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn committed_fgd_is_current() {
    let committed = std::fs::read_to_string(root().join("trenchbroom/RetroFPS.fgd")).unwrap();

    assert!(
        committed == entities::fgd(),
        "trenchbroom/RetroFPS.fgd is stale: regenerate it"
    );
}

#[test]
fn game_config_names_the_fgd_and_its_tool_textures_exist() {
    let config = std::fs::read_to_string(root().join("trenchbroom/GameConfig.cfg")).unwrap();
    assert!(config.contains("\"definitions\": [ \"RetroFPS.fgd\" ]"));
    assert!(config.contains("\"searchpath\": \"assets\""));

    for name in TOOL_TEXTURES {
        assert!(config.contains(&format!("\"pattern\": \"{name}")) || name == "trigger");
        let png = std::fs::read(root().join(format!("assets/textures/{name}.png"))).unwrap();
        assert!(png.starts_with(PNG_SIGNATURE), "{name}.png");
        assert!(level::is_tool_texture(name));
    }
}
