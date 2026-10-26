//! Every entity class the engine and its tools understand. Adding a
//! class here puts it in the editor (via the FGD); giving it behavior
//! is the engine's job (`engine/src/map/entities.rs`), or a Lua
//! script's.

use crate::class::{Choice, Class, Key, KeyType, Kind};

/// Keys every class has.
const NAME: Key = Key {
    name: "targetname",
    kind: KeyType::Name,
    default: None,
    description: "Name: what targets this",
};
const TARGET: Key = Key {
    name: "target",
    kind: KeyType::Target,
    default: None,
    description: "Fires every entity with this name",
};
const SCRIPT: Key = Key {
    name: "script",
    kind: KeyType::Text,
    default: None,
    description: "Lua class (a table in assets/scripts/) whose hooks run for it",
};
const ORIGIN: Key = Key {
    name: "origin",
    kind: KeyType::Vector,
    default: None,
    description: "Position",
};
const SHADOW: Key = Key {
    name: "_shadow",
    kind: KeyType::Choices(&[
        Choice {
            value: "0",
            label: "No",
        },
        Choice {
            value: "1",
            label: "Yes",
        },
    ]),
    default: Some("0"),
    description: "Casts shadows when the map is lit",
};

const PLAYER_SIZE: ([i32; 3], [i32; 3]) = ([-16, -16, -24], [16, 16, 32]);
const LIGHT_SIZE: ([i32; 3], [i32; 3]) = ([-8, -8, -8], [8, 8, 8]);
const PROP_SIZE: ([i32; 3], [i32; 3]) = ([-16, -16, 0], [16, 16, 32]);
const GREEN: [u8; 3] = [0, 255, 0];
const BROWN: [u8; 3] = [160, 100, 40];
/// The key naming a prop's model file.
const MODEL_KEY: &str = "model";
/// The key the light baker writes on props: light at the origin, in
/// luxel units (128 is full brightness).
pub const BAKED_LIGHT: &str = "_light";
const YELLOW: [u8; 3] = [255, 255, 0];

/// Every class, worldspawn first.
pub const CLASSES: &[Class] = &[
    Class {
        name: "worldspawn",
        kind: Kind::Brush,
        description: "The world: every brush not in another entity",
        keys: &[
            Key {
                name: "message",
                kind: KeyType::Text,
                default: None,
                description: "Map title",
            },
            Key {
                name: "light",
                kind: KeyType::Integer,
                default: Some("0"),
                description: "Least light anywhere (minlight)",
            },
            Key {
                name: "_anglescale",
                kind: KeyType::Float,
                default: Some("0.5"),
                description: "How much light depends on its arrival angle, 0 to 1",
            },
            Key {
                name: "_bounce",
                kind: KeyType::Integer,
                default: Some("1"),
                description: "Bounced light passes (0: none)",
            },
            Key {
                name: "_bouncescale",
                kind: KeyType::Float,
                default: Some("1"),
                description: "Bounced light multiplier",
            },
        ],
    },
    Class {
        name: "info_player_start",
        kind: Kind::Point {
            size: PLAYER_SIZE,
            color: GREEN,
            model: None,
        },
        description: "Where the player starts",
        keys: &[
            ORIGIN,
            Key {
                name: "angle",
                kind: KeyType::Integer,
                default: Some("0"),
                description: "Facing, degrees counter-clockwise from +X",
            },
        ],
    },
    Class {
        name: "light",
        kind: Kind::Point {
            size: LIGHT_SIZE,
            color: YELLOW,
            model: None,
        },
        description: "Point light, baked into lightmaps",
        keys: &[
            ORIGIN,
            Key {
                name: "light",
                kind: KeyType::Integer,
                default: Some("300"),
                description: "Brightness",
            },
            Key {
                name: "wait",
                kind: KeyType::Float,
                default: Some("1"),
                description: "Distance scale: 2 reaches half as far",
            },
            Key {
                name: "delay",
                kind: KeyType::Choices(&[
                    Choice {
                        value: "0",
                        label: "Linear",
                    },
                    Choice {
                        value: "1",
                        label: "1/distance",
                    },
                    Choice {
                        value: "2",
                        label: "1/distance^2",
                    },
                ]),
                default: Some("0"),
                description: "Falloff",
            },
        ],
    },
    Class {
        name: "prop_static",
        kind: Kind::Point {
            size: PROP_SIZE,
            color: BROWN,
            model: Some(MODEL_KEY),
        },
        description: "A glTF model (.glb) standing in the world",
        keys: &[
            ORIGIN,
            NAME,
            SCRIPT,
            Key {
                name: MODEL_KEY,
                kind: KeyType::Text,
                default: None,
                description: "Model file under assets/, e.g. models/barrel.glb",
            },
            Key {
                name: "angle",
                kind: KeyType::Integer,
                default: Some("0"),
                description: "Turn, degrees counter-clockwise seen from above",
            },
            Key {
                name: "scale",
                kind: KeyType::Float,
                default: Some("1"),
                description: "Size multiplier",
            },
            Key {
                name: "solid",
                kind: KeyType::Choices(&[
                    Choice {
                        value: "0",
                        label: "No",
                    },
                    Choice {
                        value: "1",
                        label: "Yes",
                    },
                ]),
                default: Some("1"),
                description: "Blocks movement with a box around the model",
            },
            Key {
                name: BAKED_LIGHT,
                kind: KeyType::Integer,
                default: None,
                description: "Light at the origin, written by the light baker",
            },
        ],
    },
    Class {
        name: "func_door",
        kind: Kind::Brush,
        description: "Slides open when fired, or on approach when unnamed",
        keys: &[
            NAME,
            SCRIPT,
            SHADOW,
            Key {
                name: "angle",
                kind: KeyType::Integer,
                default: Some("0"),
                description: "Opening direction, degrees; -1 up, -2 down",
            },
            Key {
                name: "speed",
                kind: KeyType::Float,
                default: Some("100"),
                description: "Units per second",
            },
            Key {
                name: "wait",
                kind: KeyType::Float,
                default: Some("3"),
                description: "Seconds open before closing; -1 stays open",
            },
            Key {
                name: "lip",
                kind: KeyType::Float,
                default: Some("8"),
                description: "Units left showing when open",
            },
        ],
    },
    Class {
        name: "func_button",
        kind: Kind::Brush,
        description: "Fires its target when used",
        keys: &[NAME, TARGET, SCRIPT, SHADOW],
    },
    Class {
        name: "trigger_once",
        kind: Kind::Brush,
        description: "Fires its target the first time the player enters",
        keys: &[NAME, TARGET, SCRIPT],
    },
    Class {
        name: "trigger_multiple",
        kind: Kind::Brush,
        description: "Fires its target every time the player enters",
        keys: &[NAME, TARGET, SCRIPT],
    },
    Class {
        name: "func_wall",
        kind: Kind::Brush,
        description: "Drawn and solid, nothing more",
        keys: &[NAME, SCRIPT, SHADOW],
    },
    Class {
        name: "func_illusionary",
        kind: Kind::Brush,
        description: "Drawn, not solid",
        keys: &[NAME, SCRIPT],
    },
    Class {
        name: "func_group",
        kind: Kind::Group,
        description: "Groups brushes in the editor; part of the world",
        keys: &[],
    },
    Class {
        name: "func_detail",
        kind: Kind::Group,
        description: "Detail brushes; part of the world",
        keys: &[],
    },
];

/// The class called `name`.
pub fn class(name: &str) -> Option<&'static Class> {
    CLASSES.iter().find(|c| c.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_keys_unique_per_class() {
        for (i, class) in CLASSES.iter().enumerate() {
            assert!(
                CLASSES[i + 1..].iter().all(|c| c.name != class.name),
                "{}",
                class.name
            );
            for (k, key) in class.keys.iter().enumerate() {
                let repeated = class.keys[k + 1..]
                    .iter()
                    .any(|other| other.name == key.name);
                assert!(!repeated, "{}.{}", class.name, key.name);
            }
        }
    }

    #[test]
    fn defaults_fit_their_types() {
        for class in CLASSES {
            for key in class.keys {
                let Some(default) = key.default else {
                    continue;
                };
                let fits = match key.kind {
                    KeyType::Integer => default.parse::<i64>().is_ok(),
                    KeyType::Float => default.parse::<f64>().is_ok(),
                    KeyType::Choices(choices) => choices.iter().any(|c| c.value == default),
                    _ => true,
                };
                assert!(fits, "{}.{} = {default:?}", class.name, key.name);
            }
        }
    }

    #[test]
    fn lookups_find_classes_and_keys() {
        let door = class("func_door").unwrap();

        assert_eq!(door.key("lip").unwrap().default, Some("8"));
        assert!(door.key("nope").is_none() && class("nope").is_none());
        assert_eq!(CLASSES[0].name, "worldspawn");
        assert_eq!(class("func_detail").unwrap().kind, Kind::Group);
    }
}
