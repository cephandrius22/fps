//! The entity lump: Quake's text list of key/value blocks.
//!
//! ```text
//! {
//! "classname" "func_door"
//! "targetname" "door1"
//! "model" "*1"
//! }
//! ```

use level::Entity;

use crate::error::{BspError, Result};

/// Parses the entity text. Strings have no escapes; anything outside
/// quotes other than braces and whitespace is an error.
pub(crate) fn parse(text: &str) -> Result<Vec<Entity>> {
    let mut tokens = Tokens::new(text);
    let mut entities = Vec::new();

    while let Some(token) = tokens.next()? {
        if token != Token::Open {
            return Err(tokens.error("expected '{'"));
        }

        let mut fields = Vec::new();
        loop {
            match tokens.next()? {
                Some(Token::Close) => break,
                Some(Token::Text(key)) => {
                    let Some(Token::Text(value)) = tokens.next()? else {
                        return Err(tokens.error("expected a value"));
                    };
                    fields.push((key.to_owned(), value.to_owned()));
                }
                _ => return Err(tokens.error("expected a key or '}'")),
            }
        }
        entities.push(Entity::new(fields));
    }

    Ok(entities)
}

#[derive(Debug, PartialEq)]
enum Token<'a> {
    Open,
    Close,
    Text(&'a str),
}

struct Tokens<'a> {
    rest: &'a str,
    line: usize,
}

impl<'a> Tokens<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            rest: text,
            line: 1,
        }
    }

    fn error(&self, message: &'static str) -> BspError {
        BspError::Entities {
            line: self.line,
            message,
        }
    }

    /// Next token; None at the end. A trailing NUL counts as the end.
    fn next(&mut self) -> Result<Option<Token<'a>>> {
        let trimmed = self
            .rest
            .trim_start_matches(|c: char| c.is_whitespace() || c == '\0');
        self.line += self.rest[..self.rest.len() - trimmed.len()]
            .matches('\n')
            .count();
        self.rest = trimmed;

        let Some(first) = self.rest.chars().next() else {
            return Ok(None);
        };
        let token = match first {
            '{' => Token::Open,
            '}' => Token::Close,
            '"' => {
                let body = &self.rest[1..];
                let len = body
                    .find('"')
                    .ok_or_else(|| self.error("unterminated string"))?;
                self.line += body[..len].matches('\n').count();
                self.rest = &body[len + 1..];
                return Ok(Some(Token::Text(&body[..len])));
            }
            _ => return Err(self.error("unexpected character")),
        };

        self.rest = &self.rest[1..];
        Ok(Some(token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"func_door\"\n\"model\" \"*3\"\n\"wait\" \"1\"\n\"wait\" \"2\"\n}\n\0";

    #[test]
    fn parses_blocks_and_pairs() {
        let entities = parse(TWO).unwrap();

        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].classname(), "worldspawn");
        assert_eq!(entities[1].get("model"), Some("*3"));
        assert_eq!(entities[1].fields().count(), 4);
    }

    #[test]
    fn last_repeated_key_wins() {
        assert_eq!(parse(TWO).unwrap()[1].get("wait"), Some("2"));
    }

    #[test]
    fn model_index_comes_from_star_n_or_worldspawn() {
        let entities = parse(TWO).unwrap();

        assert_eq!(entities[0].model(), Some(0));
        assert_eq!(entities[1].model(), Some(3));
    }

    #[test]
    fn errors_report_the_line() {
        let err = parse("{\n\"a\" \"b\"\n\"dangling\"\n}").unwrap_err();
        assert!(matches!(err, BspError::Entities { line: 4, .. }), "{err:?}");

        assert!(parse("{ \"open").is_err());
        assert!(parse("x").is_err());
    }
}
