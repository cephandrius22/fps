//! The catalog as a Forge Game Data (.fgd) file: how TrenchBroom learns
//! which entities exist, their keys, and how to draw them.
//!
//! ```text
//!   @PointClass size(-8 -8 -8, 8 8 8) color(255 255 0) = light : "Point light…"
//!   [
//!       light(integer) : "Brightness" : 300
//!       delay(choices) : "Falloff" : 0 =
//!       [
//!           0 : "Linear"
//!       ]
//!   ]
//! ```
//!
//! Left out: "origin" (the editor places point entities itself) and
//! keys the compiler writes.

use std::fmt::Write;

use crate::catalog::{BAKED_LIGHT, CLASSES};
use crate::class::{Class, Key, KeyType, Kind};

const HEADER: &str = "// Entity definitions for TrenchBroom. Generated from \
crates/entities/src/catalog.rs: edit that, then run\n\
// cargo run -p entities -- trenchbroom/RetroFPS.fgd\n";
/// Keys the editor or the compiler sets, never the mapper.
const HIDDEN_KEYS: [&str; 2] = ["origin", BAKED_LIGHT];
const INDENT: &str = "\t";

/// The whole FGD text.
pub fn fgd() -> String {
    let mut out = String::from(HEADER);
    for class in CLASSES {
        out.push('\n');
        write_class(&mut out, class);
    }
    out
}

fn write_class(out: &mut String, class: &Class) {
    let header = match class.kind {
        Kind::Point { size, color, model } => {
            let [x0, y0, z0] = size.0;
            let [x1, y1, z1] = size.1;
            let [r, g, b] = color;
            // TrenchBroom draws the file this key names.
            let model = model
                .map(|key| format!(" model({{ \"path\": {key} }})"))
                .unwrap_or_default();
            format!("@PointClass size({x0} {y0} {z0}, {x1} {y1} {z1}) color({r} {g} {b}){model}")
        }
        Kind::Brush | Kind::Group => "@SolidClass".to_owned(),
    };
    // Writing to a String cannot fail.
    let _ = writeln!(out, "{header} = {} : \"{}\"", class.name, class.description);

    out.push_str("[\n");
    for key in class.keys.iter().filter(|k| !HIDDEN_KEYS.contains(&k.name)) {
        write_key(out, key);
    }
    out.push_str("]\n");
}

fn write_key(out: &mut String, key: &Key) {
    let (kind, quote) = match key.kind {
        KeyType::Text | KeyType::Vector => ("string", Quote::Yes),
        KeyType::Integer => ("integer", Quote::No),
        KeyType::Float => ("float", Quote::Yes),
        KeyType::Name => ("target_source", Quote::Yes),
        KeyType::Target => ("target_destination", Quote::Yes),
        KeyType::Choices(_) => ("choices", Quote::No),
    };
    let _ = write!(
        out,
        "{INDENT}{}({kind}) : \"{}\"",
        key.name, key.description
    );

    if let Some(default) = key.default {
        match quote {
            Quote::Yes => {
                let _ = write!(out, " : \"{default}\"");
            }
            Quote::No => {
                let _ = write!(out, " : {default}");
            }
        }
    }

    let KeyType::Choices(choices) = key.kind else {
        out.push('\n');
        return;
    };
    let _ = writeln!(out, " =\n{INDENT}[");
    for choice in choices {
        let _ = writeln!(
            out,
            "{INDENT}{INDENT}{} : \"{}\"",
            choice.value, choice.label
        );
    }
    let _ = writeln!(out, "{INDENT}]");
}

/// Whether a default is written in quotes.
enum Quote {
    Yes,
    No,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_point_and_solid_classes_with_their_keys() {
        let text = fgd();

        assert!(text.starts_with("// Entity definitions"));
        assert!(text.contains(
            "@PointClass size(-8 -8 -8, 8 8 8) color(255 255 0) = light : \"Point light, baked into lightmaps\"\n["
        ));
        assert!(text.contains("@SolidClass = func_door : "));
        assert!(text.contains("\tlip(float) : \"Units left showing when open\" : \"8\"\n"));
        assert!(text.contains("\tlight(integer) : \"Brightness\" : 300\n"));
        assert!(text.contains("\ttarget(target_destination) : "));
        assert!(text.contains("\tdelay(choices) : \"Falloff\" : 0 =\n\t[\n\t\t0 : \"Linear\"\n"));
        assert!(!text.contains("origin("), "the editor's own key");
        assert!(!text.contains("_light("), "the baker's key");
        assert!(text.contains("color(160 100 40) model({ \"path\": model }) = prop_static : "));
    }

    #[test]
    fn every_class_appears_once_and_brackets_balance() {
        let text = fgd();

        for class in CLASSES {
            let decl = format!(" = {} : ", class.name);
            assert_eq!(text.matches(&decl).count(), 1, "{}", class.name);
        }
        assert_eq!(text.matches('[').count(), text.matches(']').count());
    }

    #[test]
    fn descriptions_are_plain_ascii_without_quotes() {
        let texts = CLASSES.iter().flat_map(|c| {
            let keys = c.keys.iter().map(|k| k.description);
            std::iter::once(c.description).chain(keys)
        });
        let labels = CLASSES
            .iter()
            .flat_map(|c| c.keys)
            .flat_map(|k| match k.kind {
                KeyType::Choices(choices) => choices.iter().map(|c| c.label).collect(),
                _ => Vec::new(),
            });
        for text in texts.chain(labels) {
            assert!(text.is_ascii() && !text.contains('"'), "{text}");
        }
    }
}
