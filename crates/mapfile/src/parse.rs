//! The .map grammar, top down:
//!
//! ```text
//!   map    = entity*
//!   entity = '{' (pair | brush)* '}'
//!   pair   = "key" "value"
//!   brush  = '{' face+ '}'
//!   face   = point point point texture mapping
//!   point  = '(' x y z ')'
//!   mapping (standard)  = x-offset y-offset rotation x-scale y-scale
//!   mapping (Valve 220) = '[' sx sy sz s-offset ']' '[' tx ty tz t-offset ']'
//!                         rotation s-scale t-scale
//! ```

use glam::{DVec2, DVec3};

use crate::error::Result;
use crate::lexer::Lexer;
use crate::model::{Axes, Brush, Entity, Face, Map};

const OPEN: char = '{';
const CLOSE: char = '}';
const QUOTE: char = '"';
const POINT_OPEN: char = '(';
const POINT_CLOSE: char = ')';
const AXIS_OPEN: char = '[';
const AXIS_CLOSE: char = ']';

pub(crate) fn map(text: &str) -> Result<Map> {
    let mut lexer = Lexer::new(text);
    let mut entities = Vec::new();

    while lexer.peek().is_some() {
        entities.push(entity(&mut lexer)?);
    }

    Ok(Map { entities })
}

fn entity(lexer: &mut Lexer) -> Result<Entity> {
    let line = lexer.line();
    lexer.expect(OPEN, "expected '{' to start an entity")?;

    let mut fields = Vec::new();
    let mut brushes = Vec::new();
    loop {
        match lexer.peek() {
            Some(QUOTE) => {
                let key = lexer.quoted()?.to_owned();
                let value = lexer.quoted()?.to_owned();
                fields.push((key, value));
            }
            Some(OPEN) => brushes.push(brush(lexer)?),
            Some(CLOSE) => break,
            None => return Err(lexer.error("unexpected end of file in an entity")),
            Some(_) => return Err(lexer.error("expected a key, a brush or '}'")),
        }
    }
    lexer.expect(CLOSE, "expected '}'")?;

    Ok(Entity {
        fields: level::Entity::new(fields),
        brushes,
        line,
    })
}

fn brush(lexer: &mut Lexer) -> Result<Brush> {
    let line = lexer.line();
    lexer.expect(OPEN, "expected '{' to start a brush")?;

    let mut faces = Vec::new();
    while !lexer.eat(CLOSE) {
        faces.push(face(lexer)?);
    }
    if faces.is_empty() {
        return Err(lexer.error("brush has no faces"));
    }

    Ok(Brush { faces, line })
}

fn face(lexer: &mut Lexer) -> Result<Face> {
    let points = [point(lexer)?, point(lexer)?, point(lexer)?];
    let texture = lexer.word()?.to_owned();

    // Valve 220 gives each axis in brackets; standard goes straight to
    // the offsets.
    let (axes, offset) = match lexer.peek() {
        Some(AXIS_OPEN) => {
            let (s, s_offset) = axis(lexer)?;
            let (t, t_offset) = axis(lexer)?;
            (Axes::Valve([s, t]), DVec2::new(s_offset, t_offset))
        }
        _ => (Axes::Standard, DVec2::new(lexer.number()?, lexer.number()?)),
    };
    let rotation = lexer.number()?;
    let scale = DVec2::new(lexer.number()?, lexer.number()?);

    Ok(Face {
        points,
        texture,
        axes,
        offset,
        rotation,
        scale,
    })
}

fn point(lexer: &mut Lexer) -> Result<DVec3> {
    lexer.expect(POINT_OPEN, "expected '(' to start a point")?;
    let p = DVec3::new(lexer.number()?, lexer.number()?, lexer.number()?);
    lexer.expect(POINT_CLOSE, "expected ')' to end a point")?;
    Ok(p)
}

/// A Valve 220 axis and its offset: `[ x y z offset ]`.
fn axis(lexer: &mut Lexer) -> Result<(DVec3, f64)> {
    lexer.expect(AXIS_OPEN, "expected '['")?;
    let axis = DVec3::new(lexer.number()?, lexer.number()?, lexer.number()?);
    let offset = lexer.number()?;
    lexer.expect(AXIS_CLOSE, "expected ']'")?;
    Ok((axis, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MapError;

    const STANDARD: &str = "\
// Game: Quake
{
\"classname\" \"worldspawn\"
\"message\" \"Test\"
{
( 0 0 0 ) ( 0 64 0 ) ( 64 0 0 ) floor 8 -4 90 0.5 2
( 0 0 64 ) ( 64 0 64 ) ( 0 64 64 ) {grate 0 0 0 1 1
}
}
{
\"classname\" \"light\"
\"origin\" \"0 0 32\"
}
";

    const VALVE: &str = "\
{
\"classname\" \"worldspawn\"
\"mapversion\" \"220\"
{
( 0 0 0 ) ( 0 64 0 ) ( 64 0 0 ) floor [ 1 0 0 16 ] [ 0 -1 0 -8 ] 45 1 1
}
}
";

    #[test]
    fn reads_entities_brushes_and_standard_faces() {
        let map = map(STANDARD).unwrap();

        assert_eq!(map.entities.len(), 2);
        let world = &map.entities[0];
        assert_eq!(world.fields.classname(), "worldspawn");
        assert_eq!(world.fields.get("message"), Some("Test"));
        assert_eq!((world.line, world.brushes[0].line), (2, 5));

        let face = &world.brushes[0].faces[0];
        assert_eq!(face.points[1], DVec3::new(0.0, 64.0, 0.0));
        assert_eq!(face.texture, "floor");
        assert_eq!(face.axes, Axes::Standard);
        assert_eq!(face.offset, DVec2::new(8.0, -4.0));
        assert_eq!((face.rotation, face.scale), (90.0, DVec2::new(0.5, 2.0)));
        assert_eq!(world.brushes[0].faces[1].texture, "{grate");

        let light = &map.entities[1];
        assert!(light.brushes.is_empty());
        assert_eq!(light.fields.get("origin"), Some("0 0 32"));
    }

    #[test]
    fn reads_valve_220_axes() {
        let map = map(VALVE).unwrap();
        let face = &map.entities[0].brushes[0].faces[0];

        assert_eq!(face.axes, Axes::Valve([DVec3::X, DVec3::NEG_Y]));
        assert_eq!(face.offset, DVec2::new(16.0, -8.0));
        assert_eq!((face.rotation, face.scale), (45.0, DVec2::ONE));
    }

    #[test]
    fn empty_text_is_an_empty_map() {
        assert_eq!(map("// nothing\n").unwrap(), Map::default());
    }

    #[test]
    fn syntax_errors_name_their_line() {
        let cases = [
            ("x", 1, "expected '{' to start an entity"),
            ("{\n\"classname\"", 2, "expected '\"'"),
            ("{\n\"a\" \"b\"\n", 3, "unexpected end of file in an entity"),
            ("{\nfoo\n}", 2, "expected a key, a brush or '}'"),
            ("{\n{\n}\n}", 3, "brush has no faces"),
            ("{\n{\n( 0 0 ) ( 0 0 0 )", 3, "expected a number"),
            ("{\n{\n( 0 0 0 ( 0 0 0 )", 3, "expected ')' to end a point"),
            (
                "{{( 0 0 0 ) ( 0 0 0 ) ( 0 0 0 ) a [ 1 0 0 0 0 ]",
                1,
                "expected ']'",
            ),
            (
                "{{( 0 0 0 ) ( 0 0 0 ) ( 0 0 0 ) a 0 0 0 1",
                1,
                "unexpected end of file",
            ),
        ];

        for (text, line, message) in cases {
            assert_eq!(map(text), Err(MapError { line, message }), "{text:?}");
        }
    }
}
