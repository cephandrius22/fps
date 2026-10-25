//! Characters to tokens. Context decides what a token is: a texture
//! name may start with '{' (Quake's transparent textures), so the
//! parser asks for the kind it expects rather than the lexer guessing.

use crate::error::{MapError, Result};

/// Starts a comment that runs to the end of the line.
const COMMENT: &str = "//";
const QUOTE: char = '"';

/// Ends a number: whitespace or punctuation, e.g. the ')' in "64)".
fn ends_number(c: char) -> bool {
    c.is_whitespace() || "(){}[]\"".contains(c)
}

/// A cursor over .map text that counts lines.
pub(crate) struct Lexer<'a> {
    rest: &'a str,
    line: usize,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            rest: text,
            line: 1,
        }
    }

    /// Current 1-based line.
    pub(crate) fn line(&self) -> usize {
        self.line
    }

    /// An error at the current line.
    pub(crate) fn error(&self, message: &'static str) -> MapError {
        MapError {
            line: self.line,
            message,
        }
    }

    /// The next character after whitespace and comments; None at the end.
    pub(crate) fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.rest.chars().next()
    }

    /// Consumes `c` if it is next.
    pub(crate) fn eat(&mut self, c: char) -> bool {
        if self.peek() != Some(c) {
            return false;
        }
        self.advance(c.len_utf8());
        true
    }

    /// Consumes `c`, or fails with `message`.
    pub(crate) fn expect(&mut self, c: char, message: &'static str) -> Result<()> {
        if self.eat(c) {
            return Ok(());
        }
        Err(self.error(message))
    }

    /// A quoted string, without its quotes. No escapes; it may not span
    /// lines.
    pub(crate) fn quoted(&mut self) -> Result<&'a str> {
        self.expect(QUOTE, "expected '\"'")?;
        let Some(len) = self.rest.find(['"', '\n']) else {
            return Err(self.error("unterminated string"));
        };
        if !self.rest[len..].starts_with(QUOTE) {
            return Err(self.error("unterminated string"));
        }

        let text = &self.rest[..len];
        self.advance(len + QUOTE.len_utf8());
        Ok(text)
    }

    /// A name: quoted, or a run of anything but whitespace.
    pub(crate) fn word(&mut self) -> Result<&'a str> {
        match self.peek() {
            None => Err(self.error("unexpected end of file")),
            Some(QUOTE) => self.quoted(),
            Some(_) => Ok(self.run(char::is_whitespace)),
        }
    }

    /// A decimal number, e.g. "-16" or "0.5".
    pub(crate) fn number(&mut self) -> Result<f64> {
        if self.peek().is_none() {
            return Err(self.error("unexpected end of file"));
        }
        let text = self.run(ends_number);
        text.parse().map_err(|_| self.error("expected a number"))
    }

    /// Consumes characters up to the first that `ends`.
    fn run(&mut self, ends: fn(char) -> bool) -> &'a str {
        let len = self.rest.find(ends).unwrap_or(self.rest.len());
        let text = &self.rest[..len];
        self.advance(len);
        text
    }

    fn skip_space(&mut self) {
        loop {
            let trimmed = self.rest.trim_start();
            self.advance(self.rest.len() - trimmed.len());
            if !self.rest.starts_with(COMMENT) {
                return;
            }

            let len = self.rest.find('\n').unwrap_or(self.rest.len());
            self.advance(len);
        }
    }

    /// Moves past `len` bytes, counting the newlines in them.
    fn advance(&mut self, len: usize) {
        self.line += self.rest[..len].matches('\n').count();
        self.rest = &self.rest[len..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_comments_and_counts_lines() {
        let mut lexer = Lexer::new("// title\n\n  // more\n  {");

        assert_eq!(lexer.peek(), Some('{'));
        assert_eq!(lexer.line(), 4);
    }

    #[test]
    fn numbers_end_at_punctuation() {
        let mut lexer = Lexer::new("( -16 0.5)");

        assert!(lexer.eat('('));
        assert_eq!(lexer.number(), Ok(-16.0));
        assert_eq!(lexer.number(), Ok(0.5));
        assert!(lexer.eat(')'));
        assert_eq!(lexer.peek(), None);
    }

    #[test]
    fn words_may_start_with_punctuation_or_be_quoted() {
        let mut lexer = Lexer::new("{grate *water1 \"two words\"");

        assert_eq!(lexer.word(), Ok("{grate"));
        assert_eq!(lexer.word(), Ok("*water1"));
        assert_eq!(lexer.word(), Ok("two words"));
    }

    #[test]
    fn bad_strings_and_numbers_are_errors_on_their_line() {
        let strings = [("\n\"open", 2), ("\"a\nb\"", 1)];
        for (text, line) in strings {
            let err = Lexer::new(text).quoted().unwrap_err();
            let message = "unterminated string";
            assert_eq!(err, MapError { line, message }, "{text:?}");
        }

        let numbers = [("x", "expected a number"), ("", "unexpected end of file")];
        for (text, message) in numbers {
            let err = Lexer::new(text).number().unwrap_err();
            assert_eq!(err, MapError { line: 1, message }, "{text:?}");
        }
    }
}
