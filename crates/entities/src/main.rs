//! `fgd`: writes the entity catalog as a TrenchBroom FGD file.
//!
//! ```text
//!   cargo run -p entities -- trenchbroom/RetroFPS.fgd
//! ```

use anyhow::{Context, Result};

const USAGE: &str = "usage: fgd <output.fgd>";

fn main() -> Result<()> {
    let path = std::env::args().nth(1).context(USAGE)?;
    std::fs::write(&path, entities::fgd()).with_context(|| format!("write {path}"))?;
    println!("wrote {path}");
    Ok(())
}
