use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use regex::Regex;
use serde_json::{Number, Value};

use crate::error::PipelineError;
use crate::selector::{select_values, Selector};
use crate::value_cast::{cast_value, CastValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueCast {
    String,
    Number,
    Boolean,
    Ip,
    DateTime,
    Version,
    Natural,
}

impl Display for ValueCast {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::String => "str",
            Self::Number => "num",
            Self::Boolean => "bool",
            Self::Ip => "ip",
            Self::DateTime => "datetime",
            Self::Version => "version",
            Self::Natural => "natural",
        })
    }
}

impl FromStr for ValueCast {
    type Err = PipelineError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        match source.to_ascii_lowercase().as_str() {
            "str" => Ok(Self::String),
            "num" => Ok(Self::Number),
            "bool" => Ok(Self::Boolean),
            "ip" => Ok(Self::Ip),
            "datetime" => Ok(Self::DateTime),
            "version" => Ok(Self::Version),
            "natural" => Ok(Self::Natural),
            _ => Err(PipelineError::Parse(format!(
                "Unknown typed predicate cast '{source}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedLiteral {
    String(String),
    Number(Number),
    Boolean(bool),
    Null,
}

impl TypedLiteral {
    fn as_json(&self) -> Value {
        match self {
            Self::String(value) => Value::String(value.clone()),
            Self::Number(value) => Value::Number(value.clone()),
            Self::Boolean(value) => Value::Bool(*value),
            Self::Null => Value::Null,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredicateOperator {
    Compare {
        comparison: Comparison,
        literal: TypedLiteral,
    },
    Regex {
        pattern: String,
        negated: bool,
    },
    In {
        literals: Vec<TypedLiteral>,
        negated: bool,
    },
    IsNull {
        negated: bool,
    },
    IsMissing {
        negated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredicateTest {
    selector: Selector,
    cast: Option<ValueCast>,
    operator: PredicateOperator,
}

impl PredicateTest {
    pub fn selector(&self) -> &Selector {
        &self.selector
    }

    pub fn cast(&self) -> Option<ValueCast> {
        self.cast
    }

    pub fn operator(&self) -> &PredicateOperator {
        &self.operator
    }

    fn validate(self, position: usize) -> Result<Self, PipelineError> {
        match (&self.operator, self.cast) {
            (PredicateOperator::Regex { pattern, .. }, cast) => {
                Regex::new(pattern).map_err(|error| {
                    parse_error(position, format!("Invalid predicate regex: {error}"))
                })?;
                if cast.is_some_and(|cast| cast != ValueCast::String) {
                    return Err(parse_error(position, "Regex predicates only accept AS str"));
                }
            }
            (PredicateOperator::Compare { literal, .. }, Some(cast)) => {
                validate_literal_cast(literal, cast, position)?;
            }
            (PredicateOperator::In { literals, .. }, Some(cast)) => {
                for literal in literals {
                    validate_literal_cast(literal, cast, position)?;
                }
            }
            _ => {}
        }
        Ok(self)
    }

    fn matches(&self, value: &Value, row: usize, stage: &str) -> Result<bool, PipelineError> {
        let selected = select_values(value, &self.selector);
        if matches!(
            &self.operator,
            PredicateOperator::IsNull { .. } | PredicateOperator::IsMissing { .. }
        ) {
            if let Some(cast) = self.cast {
                for value in &selected {
                    self.cast_selected(value, cast, row, stage)?;
                }
            }
        }
        match &self.operator {
            PredicateOperator::IsNull { negated } => {
                let matched = selected.iter().any(|value| value.is_null());
                Ok(if *negated { !matched } else { matched })
            }
            PredicateOperator::IsMissing { negated } => {
                let matched = selected.is_empty();
                Ok(if *negated { !matched } else { matched })
            }
            PredicateOperator::Regex { pattern, negated } => {
                let regex =
                    Regex::new(pattern).expect("predicate regex is validated during parsing");
                if selected.is_empty() {
                    return Ok(false);
                }
                let values = selected
                    .iter()
                    .map(|value| self.regex_text(value, row, stage))
                    .collect::<Result<Vec<_>, _>>()?;
                if *negated {
                    Ok(values
                        .iter()
                        .all(|value| value.as_ref().is_none_or(|value| !regex.is_match(value))))
                } else {
                    Ok(values
                        .iter()
                        .any(|value| value.as_ref().is_some_and(|value| regex.is_match(value))))
                }
            }
            PredicateOperator::Compare {
                comparison,
                literal,
            } => {
                let negative = *comparison == Comparison::NotEqual;
                if selected.is_empty() {
                    return Ok(false);
                }
                let results = selected
                    .iter()
                    .map(|value| self.compare(value, literal, *comparison, row, stage))
                    .collect::<Result<Vec<_>, _>>()?;
                if negative {
                    Ok(!results.is_empty() && results.into_iter().all(|matched| matched))
                } else {
                    Ok(results.into_iter().any(|matched| matched))
                }
            }
            PredicateOperator::In { literals, negated } => {
                if selected.is_empty() {
                    return Ok(false);
                }
                let results = selected
                    .iter()
                    .map(|value| self.in_list(value, literals, row, stage))
                    .collect::<Result<Vec<_>, _>>()?;
                if *negated {
                    Ok(!results.is_empty() && results.into_iter().all(|matched| !matched))
                } else {
                    Ok(results.into_iter().any(|matched| matched))
                }
            }
        }
    }

    fn compare(
        &self,
        value: &Value,
        literal: &TypedLiteral,
        comparison: Comparison,
        row: usize,
        stage: &str,
    ) -> Result<bool, PipelineError> {
        let literal_json = literal.as_json();
        let ordering = if value.is_null() || literal_json.is_null() {
            compare_json(value, &literal_json)
        } else if let Some(cast) = self.cast {
            let Some(value) = self.cast_selected(value, cast, row, stage)? else {
                return Ok(false);
            };
            let literal = cast_value(&literal_json, cast)
                .expect("typed predicate literal cast is validated during parsing")
                .expect("non-null cast literals produce a value");
            value.compare(&literal)
        } else {
            compare_json(value, &literal_json)
        };

        Ok(match comparison {
            Comparison::Equal => ordering == Some(Ordering::Equal),
            Comparison::NotEqual => ordering != Some(Ordering::Equal),
            Comparison::Less => ordering == Some(Ordering::Less),
            Comparison::LessOrEqual => {
                matches!(ordering, Some(Ordering::Less | Ordering::Equal))
            }
            Comparison::Greater => ordering == Some(Ordering::Greater),
            Comparison::GreaterOrEqual => {
                matches!(ordering, Some(Ordering::Greater | Ordering::Equal))
            }
        })
    }

    fn in_list(
        &self,
        value: &Value,
        literals: &[TypedLiteral],
        row: usize,
        stage: &str,
    ) -> Result<bool, PipelineError> {
        if value.is_null() {
            return Ok(literals
                .iter()
                .any(|literal| matches!(literal, TypedLiteral::Null)));
        }
        if let Some(cast) = self.cast {
            let Some(value) = self.cast_selected(value, cast, row, stage)? else {
                return Ok(false);
            };
            Ok(literals
                .iter()
                .filter(|literal| !matches!(literal, TypedLiteral::Null))
                .any(|literal| {
                    let literal = cast_value(&literal.as_json(), cast)
                        .expect("typed predicate literal cast is validated during parsing")
                        .expect("non-null cast literals produce a value");
                    value == literal
                }))
        } else {
            Ok(literals.iter().any(|literal| value == &literal.as_json()))
        }
    }

    fn cast_selected(
        &self,
        value: &Value,
        cast: ValueCast,
        row: usize,
        stage: &str,
    ) -> Result<Option<CastValue>, PipelineError> {
        cast_value(value, cast).map_err(|reason| {
            PipelineError::Pipe(format!(
                "Pipe stage '{stage}' could not cast selector '{}' AS {cast} at row {row}: {reason}; offending value {value}",
                self.selector
            ))
        })
    }

    fn regex_text(
        &self,
        value: &Value,
        row: usize,
        stage: &str,
    ) -> Result<Option<String>, PipelineError> {
        match self.cast {
            Some(ValueCast::String) => {
                let Some(value) = self.cast_selected(value, ValueCast::String, row, stage)? else {
                    return Ok(None);
                };
                let CastValue::String(value) = value else {
                    unreachable!("AS str always returns a string cast value")
                };
                Ok(Some(value))
            }
            None => Ok(value.as_str().map(str::to_string)),
            Some(_) => unreachable!("regex casts are validated during parsing"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredicateExpr {
    Or(Box<Self>, Box<Self>),
    And(Box<Self>, Box<Self>),
    Not(Box<Self>),
    Test(PredicateTest),
}

impl PredicateExpr {
    fn matches(&self, value: &Value, row: usize, stage: &str) -> Result<bool, PipelineError> {
        match self {
            Self::Or(left, right) => {
                if left.matches(value, row, stage)? {
                    Ok(true)
                } else {
                    right.matches(value, row, stage)
                }
            }
            Self::And(left, right) => {
                if !left.matches(value, row, stage)? {
                    Ok(false)
                } else {
                    right.matches(value, row, stage)
                }
            }
            Self::Not(expression) => Ok(!expression.matches(value, row, stage)?),
            Self::Test(test) => test.matches(value, row, stage),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    expression: PredicateExpr,
}

impl Predicate {
    pub fn parse(source: &str) -> Result<Self, PipelineError> {
        let tokens = lex(source)?;
        let mut parser = Parser::new(tokens, source.len());
        let expression = parser.parse_expression()?;
        parser.expect_end()?;
        Ok(Self { expression })
    }

    pub fn expression(&self) -> &PredicateExpr {
        &self.expression
    }

    pub(crate) fn matches(
        &self,
        value: &Value,
        row: usize,
        stage: &str,
    ) -> Result<bool, PipelineError> {
        self.expression.matches(value, row, stage)
    }
}

impl FromStr for Predicate {
    type Err = PipelineError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::parse(source)
    }
}

fn validate_literal_cast(
    literal: &TypedLiteral,
    cast: ValueCast,
    position: usize,
) -> Result<(), PipelineError> {
    if matches!(literal, TypedLiteral::Null) {
        return Ok(());
    }
    match cast_value(&literal.as_json(), cast) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => unreachable!("non-null literals always produce or reject a cast"),
        Err(reason) => Err(parse_error(
            position,
            format!("Invalid AS {cast} literal {}: {reason}", literal.as_json()),
        )),
    }
}

fn compare_json(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(Ordering::Equal),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => compare_json_numbers(left, right),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        (Value::Array(left), Value::Array(right)) if left == right => Some(Ordering::Equal),
        (Value::Object(left), Value::Object(right)) if left == right => Some(Ordering::Equal),
        _ => None,
    }
}

fn compare_json_numbers(left: &Number, right: &Number) -> Option<Ordering> {
    match (
        (left.is_i64(), left.is_u64(), left.is_f64()),
        (right.is_i64(), right.is_u64(), right.is_f64()),
    ) {
        ((true, _, false), (true, _, false)) => Some(left.as_i64()?.cmp(&right.as_i64()?)),
        ((false, true, false), (false, true, false)) => Some(left.as_u64()?.cmp(&right.as_u64()?)),
        ((true, _, false), (false, true, false)) => {
            let left = left.as_i64()?;
            Some(if left < 0 {
                Ordering::Less
            } else {
                (left as u64).cmp(&right.as_u64()?)
            })
        }
        ((false, true, false), (true, _, false)) => {
            compare_json_numbers(right, left).map(Ordering::reverse)
        }
        _ => left.as_f64()?.partial_cmp(&right.as_f64()?),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    kind: TokenKind,
    position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Word(String),
    String(String),
    Operator(String),
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
}

fn lex(source: &str) -> Result<Vec<Token>, PipelineError> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let ch = source[offset..]
            .chars()
            .next()
            .expect("offset is inside source");
        if ch.is_whitespace() {
            offset += ch.len_utf8();
            continue;
        }

        let position = offset;
        match ch {
            '"' => {
                let (value, end) = lex_double_quoted(source, offset)?;
                tokens.push(Token {
                    kind: TokenKind::String(value),
                    position,
                });
                offset = end;
            }
            '\'' => {
                let (value, end) = lex_single_quoted(source, offset)?;
                tokens.push(Token {
                    kind: TokenKind::String(value),
                    position,
                });
                offset = end;
            }
            '(' => push_punctuation(&mut tokens, TokenKind::LeftParen, position, &mut offset),
            ')' => push_punctuation(&mut tokens, TokenKind::RightParen, position, &mut offset),
            '[' => push_punctuation(&mut tokens, TokenKind::LeftBracket, position, &mut offset),
            ']' => push_punctuation(&mut tokens, TokenKind::RightBracket, position, &mut offset),
            ',' => push_punctuation(&mut tokens, TokenKind::Comma, position, &mut offset),
            '=' | '<' | '>' | '~' | '!' => {
                let end = operator_end(source, offset);
                tokens.push(Token {
                    kind: TokenKind::Operator(source[offset..end].to_string()),
                    position,
                });
                offset = end;
            }
            _ => {
                let end = word_end(source, offset);
                if end == offset {
                    return Err(parse_error(position, "Unexpected predicate token"));
                }
                tokens.push(Token {
                    kind: TokenKind::Word(source[offset..end].to_string()),
                    position,
                });
                offset = end;
            }
        }
    }
    if tokens.is_empty() {
        return Err(parse_error(0, "Typed predicate cannot be empty"));
    }
    Ok(tokens)
}

fn push_punctuation(tokens: &mut Vec<Token>, kind: TokenKind, position: usize, offset: &mut usize) {
    tokens.push(Token { kind, position });
    *offset += 1;
}

fn lex_double_quoted(source: &str, start: usize) -> Result<(String, usize), PipelineError> {
    let mut offset = start + 1;
    let mut escaped = false;
    while offset < source.len() {
        let ch = source[offset..]
            .chars()
            .next()
            .expect("offset is inside source");
        offset += ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            let raw = &source[start..offset];
            let value = serde_json::from_str::<String>(raw).map_err(|error| {
                parse_error(start, format!("Invalid double-quoted string: {error}"))
            })?;
            return Ok((value, offset));
        }
    }
    Err(parse_error(start, "Unterminated double-quoted string"))
}

fn lex_single_quoted(source: &str, start: usize) -> Result<(String, usize), PipelineError> {
    let mut offset = start + 1;
    let mut value = String::new();
    while offset < source.len() {
        let ch = source[offset..]
            .chars()
            .next()
            .expect("offset is inside source");
        offset += ch.len_utf8();
        if ch == '\'' {
            return Ok((value, offset));
        }
        if ch == '\\' {
            let Some(next) = source[offset..].chars().next() else {
                return Err(parse_error(start, "Unterminated single-quoted string"));
            };
            if matches!(next, '\\' | '\'') {
                value.push(next);
                offset += next.len_utf8();
            } else {
                value.push('\\');
            }
        } else {
            value.push(ch);
        }
    }
    Err(parse_error(start, "Unterminated single-quoted string"))
}

fn operator_end(source: &str, start: usize) -> usize {
    let first = source.as_bytes()[start];
    let next = source.as_bytes().get(start + 1).copied();
    if matches!(
        (first, next),
        (b'=', Some(b'=')) | (b'!', Some(b'=' | b'~')) | (b'<', Some(b'=')) | (b'>', Some(b'='))
    ) {
        start + 2
    } else {
        start + 1
    }
}

fn word_end(source: &str, start: usize) -> usize {
    let mut offset = start;
    let mut selector_brackets = 0usize;
    while offset < source.len() {
        let ch = source[offset..]
            .chars()
            .next()
            .expect("offset is inside source");
        if selector_brackets == 0
            && (ch.is_whitespace()
                || matches!(ch, '(' | ')' | ']' | ',' | '=' | '<' | '>' | '~' | '!'))
        {
            break;
        }
        match ch {
            '[' => selector_brackets += 1,
            ']' if selector_brackets > 0 => selector_brackets -= 1,
            _ => {}
        }
        offset += ch.len_utf8();
    }
    offset
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    source_len: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>, source_len: usize) -> Self {
        Self {
            tokens,
            position: 0,
            source_len,
        }
    }

    fn parse_expression(&mut self) -> Result<PredicateExpr, PipelineError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<PredicateExpr, PipelineError> {
        let mut expression = self.parse_and()?;
        while self.consume_keyword("OR") {
            expression = PredicateExpr::Or(Box::new(expression), Box::new(self.parse_and()?));
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<PredicateExpr, PipelineError> {
        let mut expression = self.parse_not()?;
        while self.consume_keyword("AND") {
            expression = PredicateExpr::And(Box::new(expression), Box::new(self.parse_not()?));
        }
        Ok(expression)
    }

    fn parse_not(&mut self) -> Result<PredicateExpr, PipelineError> {
        if self.consume_keyword("NOT") {
            Ok(PredicateExpr::Not(Box::new(self.parse_not()?)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<PredicateExpr, PipelineError> {
        if self.consume_kind(&TokenKind::LeftParen) {
            let expression = self.parse_expression()?;
            self.expect_kind(
                &TokenKind::RightParen,
                "Expected ')' to close predicate group",
            )?;
            Ok(expression)
        } else {
            Ok(PredicateExpr::Test(self.parse_test()?))
        }
    }

    fn parse_test(&mut self) -> Result<PredicateTest, PipelineError> {
        let (selector_source, position) = match self.take() {
            Some(Token {
                kind: TokenKind::Word(value) | TokenKind::String(value),
                position,
            }) => (value, position),
            Some(Token {
                kind: TokenKind::LeftBracket,
                position,
            }) => (self.parse_root_selector(position)?, position),
            Some(token) => {
                return Err(parse_error(
                    token.position,
                    "Expected a selector at the start of a predicate test",
                ));
            }
            None => {
                return Err(parse_error(self.source_len, "Expected a predicate test"));
            }
        };
        let selector = Selector::new(&selector_source).map_err(|error| {
            parse_error(
                position,
                format!("Invalid predicate selector '{selector_source}': {error}"),
            )
        })?;
        let cast =
            if self.consume_keyword("AS") {
                let token = self
                    .take()
                    .ok_or_else(|| parse_error(self.source_len, "Expected a cast name after AS"))?;
                let TokenKind::Word(value) = token.kind else {
                    return Err(parse_error(token.position, "Expected a cast name after AS"));
                };
                Some(value.parse().map_err(|error: PipelineError| {
                    parse_error(token.position, error.to_string())
                })?)
            } else {
                None
            };

        let operator = if let Some(operator) = self.take_operator() {
            match operator.0.as_str() {
                "~" | "!~" => PredicateOperator::Regex {
                    pattern: self
                        .parse_string_literal("Regex predicate requires a quoted string")?,
                    negated: operator.0 == "!~",
                },
                value => PredicateOperator::Compare {
                    comparison: parse_comparison(value, operator.1)?,
                    literal: self.parse_literal()?,
                },
            }
        } else if self.consume_keyword("MATCHES") {
            PredicateOperator::Regex {
                pattern: self.parse_string_literal("MATCHES requires a quoted string")?,
                negated: false,
            }
        } else if self.consume_keyword("IS") {
            let negated = self.consume_keyword("NOT");
            if self.consume_keyword("NULL") {
                PredicateOperator::IsNull { negated }
            } else if self.consume_keyword("MISSING") {
                PredicateOperator::IsMissing { negated }
            } else {
                return Err(self.error_here("Expected NULL or MISSING after IS"));
            }
        } else {
            let negated = self.consume_keyword("NOT");
            if !self.consume_keyword("IN") {
                return Err(self.error_here("Expected a comparison, MATCHES, IN, or IS"));
            }
            PredicateOperator::In {
                literals: self.parse_literal_list()?,
                negated,
            }
        };

        PredicateTest {
            selector,
            cast,
            operator,
        }
        .validate(position)
    }

    fn parse_root_selector(&mut self, position: usize) -> Result<String, PipelineError> {
        let mut selector = String::from("[");
        self.append_selector_bracket(&mut selector, position)?;
        loop {
            if self.consume_kind(&TokenKind::LeftBracket) {
                selector.push('[');
                self.append_selector_bracket(&mut selector, position)?;
                continue;
            }
            if let Some(Token {
                kind: TokenKind::Word(suffix),
                ..
            }) = self.tokens.get(self.position)
            {
                if suffix.starts_with('.') {
                    selector.push_str(suffix);
                    self.position += 1;
                }
            }
            return Ok(selector);
        }
    }

    fn append_selector_bracket(
        &mut self,
        selector: &mut String,
        position: usize,
    ) -> Result<(), PipelineError> {
        loop {
            let token = self.take().ok_or_else(|| {
                parse_error(position, "Unterminated root-array predicate selector")
            })?;
            match token.kind {
                TokenKind::Word(value) => selector.push_str(&value),
                TokenKind::RightBracket => {
                    selector.push(']');
                    return Ok(());
                }
                _ => {
                    return Err(parse_error(
                        token.position,
                        "Invalid root-array predicate selector",
                    ));
                }
            }
        }
    }

    fn parse_literal_list(&mut self) -> Result<Vec<TypedLiteral>, PipelineError> {
        self.expect_kind(&TokenKind::LeftBracket, "Expected '[' after IN")?;
        let mut literals = Vec::new();
        if self.consume_kind(&TokenKind::RightBracket) {
            return Ok(literals);
        }
        loop {
            literals.push(self.parse_literal()?);
            if self.consume_kind(&TokenKind::RightBracket) {
                return Ok(literals);
            }
            self.expect_kind(&TokenKind::Comma, "Expected ',' between IN values")?;
            if self.peek_kind() == Some(&TokenKind::RightBracket) {
                return Err(self.error_here("Trailing commas are not allowed in IN lists"));
            }
        }
    }

    fn parse_literal(&mut self) -> Result<TypedLiteral, PipelineError> {
        let token = self
            .take()
            .ok_or_else(|| parse_error(self.source_len, "Expected a typed literal"))?;
        match token.kind {
            TokenKind::String(value) => Ok(TypedLiteral::String(value)),
            TokenKind::Word(value) if value.eq_ignore_ascii_case("true") => {
                Ok(TypedLiteral::Boolean(true))
            }
            TokenKind::Word(value) if value.eq_ignore_ascii_case("false") => {
                Ok(TypedLiteral::Boolean(false))
            }
            TokenKind::Word(value) if value.eq_ignore_ascii_case("null") => Ok(TypedLiteral::Null),
            TokenKind::Word(value) => match serde_json::from_str::<Value>(&value) {
                Ok(Value::Number(number)) => Ok(TypedLiteral::Number(number)),
                _ => Err(parse_error(
                    token.position,
                    "Typed string literals must be quoted",
                )),
            },
            _ => Err(parse_error(token.position, "Expected a typed literal")),
        }
    }

    fn parse_string_literal(&mut self, message: &str) -> Result<String, PipelineError> {
        let token = self
            .take()
            .ok_or_else(|| parse_error(self.source_len, message))?;
        match token.kind {
            TokenKind::String(value) => Ok(value),
            _ => Err(parse_error(token.position, message)),
        }
    }

    fn expect_end(&self) -> Result<(), PipelineError> {
        if let Some(token) = self.tokens.get(self.position) {
            Err(parse_error(
                token.position,
                "Unexpected extra token after complete predicate",
            ))
        } else {
            Ok(())
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        let matches = self.tokens.get(self.position).is_some_and(|token| {
            matches!(&token.kind, TokenKind::Word(value) if value.eq_ignore_ascii_case(keyword))
        });
        if matches {
            self.position += 1;
        }
        matches
    }

    fn consume_kind(&mut self, kind: &TokenKind) -> bool {
        if self.peek_kind() == Some(kind) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_kind(&mut self, kind: &TokenKind, message: &str) -> Result<(), PipelineError> {
        if self.consume_kind(kind) {
            Ok(())
        } else {
            Err(self.error_here(message))
        }
    }

    fn take_operator(&mut self) -> Option<(String, usize)> {
        let token = self.tokens.get(self.position)?;
        let TokenKind::Operator(operator) = &token.kind else {
            return None;
        };
        let result = (operator.clone(), token.position);
        self.position += 1;
        Some(result)
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.position).map(|token| &token.kind)
    }

    fn take(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position)?.clone();
        self.position += 1;
        Some(token)
    }

    fn error_here(&self, message: impl Into<String>) -> PipelineError {
        parse_error(
            self.tokens
                .get(self.position)
                .map_or(self.source_len, |token| token.position),
            message,
        )
    }
}

fn parse_comparison(value: &str, position: usize) -> Result<Comparison, PipelineError> {
    match value {
        "=" | "==" => Ok(Comparison::Equal),
        "!=" => Ok(Comparison::NotEqual),
        "<" => Ok(Comparison::Less),
        "<=" => Ok(Comparison::LessOrEqual),
        ">" => Ok(Comparison::Greater),
        ">=" => Ok(Comparison::GreaterOrEqual),
        _ => Err(parse_error(
            position,
            format!("Unknown typed comparison operator '{value}'"),
        )),
    }
}

fn parse_error(position: usize, message: impl Into<String>) -> PipelineError {
    PipelineError::Parse(format!("{} at byte {position}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::Predicate;
    use serde_json::json;

    fn matches(source: &str, value: &Value) -> Result<bool, crate::PipelineError> {
        Predicate::parse(source)?.matches(value, 1, "F WHERE")
    }

    use serde_json::Value;

    #[test]
    fn precedence_and_parentheses_are_explicit() {
        let value = json!({"a": true, "b": false, "c": false});
        assert!(matches("a == true OR b == true AND c == true", &value).expect("predicate"));
        assert!(!matches("(a == true OR b == true) AND c == true", &value).expect("predicate"));
    }

    #[test]
    fn literals_retain_json_types_and_quotes() {
        assert!(matches("value == 3", &json!({"value": 3})).expect("predicate"));
        assert!(!matches("value == 3", &json!({"value": "3"})).expect("predicate"));
        assert!(matches("value == \"3\"", &json!({"value": "3"})).expect("predicate"));
        assert!(
            matches("value == \"line\\nfeed\"", &json!({"value": "line\nfeed"}))
                .expect("predicate")
        );
    }

    #[test]
    fn fanout_and_negative_tests_follow_safe_missing_semantics() {
        let value = json!({"items": [{"state": "ready"}, {"state": "waiting"}]});
        assert!(matches("items[].state == \"ready\"", &value).expect("predicate"));
        assert!(!matches("items[].state != \"ready\"", &value).expect("predicate"));
        assert!(!matches("missing != \"ready\"", &value).expect("predicate"));
        assert!(matches("missing IS MISSING", &value).expect("predicate"));
        assert!(matches("missing IS NOT NULL", &value).expect("predicate"));
    }

    #[test]
    fn in_null_and_missing_are_distinct() {
        let value = json!({"state": null, "owner": "alice"});
        assert!(matches("owner IN [\"alice\", \"bob\"]", &value).expect("predicate"));
        assert!(matches("state IS NULL", &value).expect("predicate"));
        assert!(!matches("state IS MISSING", &value).expect("predicate"));
        assert!(matches("state AS str == null", &value).expect("cast null predicate"));
        assert!(matches("state AS str IN [\"ready\", null]", &value).expect("cast null list"));
    }

    #[test]
    fn casts_on_null_tests_validate_only_selected_non_null_values() {
        assert!(matches("state AS num IS NULL", &json!({"state": null})).expect("null"));
        assert!(matches("state AS num IS MISSING", &json!({})).expect("missing"));
        assert!(matches("state AS num IS NOT NULL", &json!({"state": "3"})).expect("number"));
        assert!(matches("state AS num IS NULL", &json!({"state": "bad"})).is_err());
    }

    #[test]
    fn root_array_selectors_use_the_shared_selector_language() {
        let value = json!([[{"state": "ready"}], [{"state": "waiting"}]]);
        assert!(matches("[][0].state == \"ready\"", &value).expect("root fanout selector"));
        assert!(matches("[0][0].state == \"ready\"", &value).expect("root index selector"));
    }

    #[test]
    fn integer_comparisons_do_not_lose_precision() {
        assert!(!matches(
            "value == 9007199254740992",
            &json!({"value": 9007199254740993_u64})
        )
        .expect("exact integer predicate"));
    }

    #[test]
    fn boolean_short_circuit_avoids_unvisited_cast_errors() {
        let value = json!({"enabled": false, "count": "invalid"});
        assert!(!matches("enabled == true AND count AS num > 3", &value).expect("short circuit"));
        assert!(matches("enabled == false OR count AS num > 3", &value).expect("short circuit"));
    }

    #[test]
    fn cast_errors_include_stage_selector_row_and_value() {
        let error = matches("count AS num > 3", &json!({"count": "invalid"}))
            .expect_err("invalid selected cast");
        let message = error.to_string();
        assert!(message.contains("F WHERE"), "{message}");
        assert!(message.contains("count"), "{message}");
        assert!(message.contains("row 1"), "{message}");
        assert!(message.contains("\"invalid\""), "{message}");
    }

    #[test]
    fn regex_string_casts_fail_strictly_for_selected_non_scalars() {
        let error = matches(
            "value AS str ~ \"needle\"",
            &json!({"value": {"nested": true}}),
        )
        .expect_err("objects cannot be cast to strings");
        let message = error.to_string();
        assert!(message.contains("F WHERE"), "{message}");
        assert!(message.contains("value"), "{message}");
        assert!(message.contains("row 1"), "{message}");
        assert!(message.contains("nested"), "{message}");
    }

    #[test]
    fn malformed_predicates_report_byte_positions() {
        for source in [
            "",
            "age >",
            "(age > 3",
            "age IN [1,]",
            "age == unquoted",
            "name MATCHES \"[\"",
            "name == \"unterminated",
            "age AS mystery > 3",
            "bad[selector == 3",
        ] {
            let error = Predicate::parse(source).expect_err("predicate should fail");
            assert!(error.to_string().contains("byte"), "{source}: {error}");
        }
    }
}
