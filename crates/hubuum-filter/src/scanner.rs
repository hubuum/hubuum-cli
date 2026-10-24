use crate::PipelineError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuoteMode {
    Shell,
    Literal,
}

#[derive(Default)]
struct QuoteState {
    quote: Option<(char, usize)>,
    escape: Option<usize>,
}

impl QuoteState {
    /// True only for an ordinary, unquoted and unescaped character.
    fn consume(&mut self, index: usize, ch: char, next: Option<char>, mode: QuoteMode) -> bool {
        if self.escape.take().is_some() {
            return false;
        }
        let escapes = match self.quote.map(|(quote, _)| quote) {
            Some('\'') => mode == QuoteMode::Literal && matches!(next, Some('\\' | '\'')),
            _ => true,
        };
        if ch == '\\' && escapes {
            self.escape = Some(index);
            return false;
        }
        match self.quote {
            Some((quote, _)) => {
                if ch == quote {
                    self.quote = None;
                }
                false
            }
            None if matches!(ch, '\'' | '"') => {
                self.quote = Some((ch, index));
                false
            }
            None => true,
        }
    }

    fn finish(self) -> Result<(), PipelineError> {
        if let Some((quote, start)) = self.quote {
            return Err(PipelineError::Parse(format!(
                "Unterminated {quote} quote at byte {start}"
            )));
        }
        if let Some(index) = self.escape {
            return Err(PipelineError::Parse(format!(
                "Trailing escape at byte {index}"
            )));
        }
        Ok(())
    }
}

fn stage_mode(source: &str) -> QuoteMode {
    let mut words = source.split_whitespace();
    if matches!(words.next(), Some("F" | "grep" | "reject"))
        && words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("WHERE"))
    {
        QuoteMode::Literal
    } else {
        QuoteMode::Shell
    }
}

/// Byte positions of unquoted delimiters, sharing the pipeline's quote rules.
/// Typed WHERE strings support escaped single quotes; command operands retain
/// shell quoting. This is also the CLI redirect scanner's integration surface.
pub fn unquoted_positions(source: &str, delimiter: char) -> Result<Vec<usize>, PipelineError> {
    let mut state = QuoteState::default();
    let mut mode = stage_mode(source);
    let mut positions = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if state.consume(index, ch, chars.peek().map(|(_, ch)| *ch), mode) {
            if ch == delimiter {
                positions.push(index);
            }
            if ch == '|' {
                mode = stage_mode(&source[index + 1..]);
            }
        }
    }
    state.finish()?;
    Ok(positions)
}

pub(crate) fn split_unquoted(source: &str, delimiter: char) -> Result<Vec<&str>, PipelineError> {
    let mut start = 0;
    let mut parts = Vec::new();
    for index in unquoted_positions(source, delimiter)? {
        parts.push(&source[start..index]);
        start = index + delimiter.len_utf8();
    }
    parts.push(&source[start..]);
    Ok(parts)
}

pub(crate) fn quoted_literal(source: &str, start: usize) -> Result<(String, usize), PipelineError> {
    let mut state = QuoteState::default();
    let mut chars = source[start..].char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        let index = start + offset;
        state.consume(
            index,
            ch,
            chars.peek().map(|(_, ch)| *ch),
            QuoteMode::Literal,
        );
        if index > start && state.quote.is_none() && state.escape.is_none() {
            let end = index + ch.len_utf8();
            if source.as_bytes()[start] == b'"' {
                return serde_json::from_str(&source[start..end])
                    .map(|value| (value, end))
                    .map_err(|error| {
                        PipelineError::Parse(format!(
                            "Invalid quoted string at byte {start}: {error}"
                        ))
                    });
            }
            let mut value = String::new();
            let mut content = source[start + 1..index].chars().peekable();
            while let Some(ch) = content.next() {
                if ch == '\\' && matches!(content.peek(), Some('\\' | '\'')) {
                    value.push(content.next().expect("peeked escape"));
                } else {
                    value.push(ch);
                }
            }
            return Ok((value, end));
        }
    }
    state.finish()?;
    Err(PipelineError::Parse(format!(
        "Expected a quote at byte {start}"
    )))
}
