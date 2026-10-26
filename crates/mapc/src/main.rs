//! `mapc`: compiles a TrenchBroom .map into a .lvl the engine loads,
//! lit by `bake`.
//!
//! ```text
//!   cargo run -p mapc -- maps/test.map assets/maps/test.lvl
//!   cargo run -p mapc -- in.map out.lvl --textures path/to/textures
//! ```
//!
//! Texture sizes and albedos come from `<textures>/<name>.png`, by
//! default `assets/textures`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

const DEFAULT_TEXTURES: &str = "assets/textures";
const TEXTURES_FLAG: &str = "--textures";
const USAGE: &str = "usage: mapc <input.map> <output.lvl> [--textures <dir>]";

/// What to compile, and where to.
struct Args {
    input: PathBuf,
    output: PathBuf,
    textures: PathBuf,
}

impl Args {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut paths = Vec::new();
        let mut textures = PathBuf::from(DEFAULT_TEXTURES);

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg != TEXTURES_FLAG {
                paths.push(PathBuf::from(arg));
                continue;
            }
            let Some(dir) = args.next() else {
                bail!("{TEXTURES_FLAG} needs a directory\n{USAGE}");
            };
            textures = dir.into();
        }

        let [input, output] =
            <[PathBuf; 2]>::try_from(paths).map_err(|_| anyhow::anyhow!(USAGE))?;
        Ok(Self {
            input,
            output,
            textures,
        })
    }
}

fn main() -> Result<()> {
    base::log::init();
    let args = Args::parse(std::env::args().skip(1))?;

    let text = std::fs::read_to_string(&args.input)
        .with_context(|| format!("read {}", args.input.display()))?;
    let map = mapfile::parse(&text).with_context(|| format!("parse {}", args.input.display()))?;
    let mut level = mapc::compile(&map, &mapc::png_sizes(&args.textures));
    let baked = bake::bake(&mut level, &mapc::png_albedos(&args.textures));
    log::info!(
        "lit {} faces ({} luxels) from {} lights, {} bounces",
        baked.faces,
        baked.luxels,
        baked.lights,
        baked.bounces
    );

    std::fs::write(&args.output, level::write(&level))
        .with_context(|| format!("write {}", args.output.display()))?;

    let faces: usize = level.models.iter().map(|m| m.faces.len()).sum();
    let brushes: usize = level.models.iter().map(|m| m.brushes.len()).sum();
    log::info!(
        "wrote {}: {} models, {faces} faces, {brushes} brushes, {} entities",
        args.output.display(),
        level.models.len(),
        level.entities.len()
    );
    Ok(())
}
