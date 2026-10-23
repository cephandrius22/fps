//! Input vocabulary shared by the platform layer (which produces it) and
//! the engine (which tracks state with it).

/// Physical keys the engine knows about. Layout-independent: WASD stays
/// WASD on AZERTY. Variant names are the key labels on a US keyboard.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Key {
    W,
    A,
    S,
    D,
    Q,
    E,
    R,
    F,
    Space,
    LShift,
    LCtrl,
    Tab,
    Escape,
    Enter,
    Up,
    Down,
    Left,
    Right,
    F1,
    F2,
    F3,
    F4,
    F12,
}

impl Key {
    /// Number of variants; sizes per-key state arrays.
    pub const COUNT: usize = Key::F12 as usize + 1;
}

/// Mouse buttons.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    /// Number of variants; sizes per-button state arrays.
    pub const COUNT: usize = Button::Middle as usize + 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_cover_last_variant() {
        assert!((Key::F12 as usize) < Key::COUNT);
        assert!((Button::Middle as usize) < Button::COUNT);
    }
}
