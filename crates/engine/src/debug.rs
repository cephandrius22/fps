//! Debug hooks: screenshots on F12, or on a fixed frame for automated
//! image checks (`ENGINE_CAPTURE_FRAME=120` saves frame 120, then exits).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use render::Renderer;

pub(crate) const SCREENSHOT_KEY: base::Key = base::Key::F12;
const SCREENSHOT_DIR: &str = "screenshots";
const CAPTURE_FRAME_ENV: &str = "ENGINE_CAPTURE_FRAME";

/// Rising-edge detector, per frame. Tick edges are consumed by the
/// game's ticks (and absent on zero-tick frames), so debug keys keep
/// their own.
#[derive(Default)]
pub(crate) struct Latch {
    held: bool,
}

impl Latch {
    /// True only on the first call after `down` turns true.
    pub fn rising(&mut self, down: bool) -> bool {
        let rose = down && !self.held;
        self.held = down;
        rose
    }
}

/// Frame number to capture from $ENGINE_CAPTURE_FRAME.
pub(crate) fn capture_frame() -> Option<u64> {
    parse_frame(std::env::var(CAPTURE_FRAME_ENV).ok().as_deref())
}

/// Frames count from 1, so 0 and garbage mean "no capture".
fn parse_frame(value: Option<&str>) -> Option<u64> {
    value?.trim().parse().ok().filter(|&n| n > 0)
}

/// Saves the last frame to screenshots/<name>.png. Logs the outcome;
/// a failed screenshot must never stop the game.
pub(crate) fn save_screenshot(renderer: &Renderer, name: &str) {
    let path = PathBuf::from(SCREENSHOT_DIR).join(format!("{name}.png"));

    let shot = match renderer.capture() {
        Ok(shot) => shot,
        Err(err) => {
            log::error!("screenshot capture failed: {}", base::chain(&err));
            return;
        }
    };

    match base::image::write_png(&path, shot.width, shot.height, &shot.rgba) {
        Ok(()) => log::info!("screenshot: {}", path.display()),
        Err(err) => log::error!("screenshot save failed: {}", base::chain(&err)),
    }
}

/// Unique-enough screenshot name: milliseconds since the Unix epoch.
pub(crate) fn timestamp_name() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latch_fires_once_per_press() {
        let mut latch = Latch::default();

        assert!(latch.rising(true));
        assert!(!latch.rising(true));
        assert!(!latch.rising(false));
        assert!(latch.rising(true));
    }

    #[test]
    fn parse_frame_accepts_positive_numbers_only() {
        assert_eq!(parse_frame(Some("120")), Some(120));
        assert_eq!(parse_frame(Some(" 7 ")), Some(7));
        assert_eq!(parse_frame(Some("0")), None);
        assert_eq!(parse_frame(Some("soon")), None);
        assert_eq!(parse_frame(None), None);
    }

    #[test]
    fn timestamp_name_is_numeric() {
        assert!(timestamp_name().chars().all(|c| c.is_ascii_digit()));
    }
}
