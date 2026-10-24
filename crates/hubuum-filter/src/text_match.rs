use regex::{escape, Regex, RegexBuilder};
use strsim::damerau_levenshtein;

use crate::PipelineError;

/// Explicit string matching modes; existing regex syntax keeps its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMatch {
    Contains,
    IContains,
    StartsWith,
    EndsWith,
    IEquals,
    Fuzzy,
}

impl TextMatch {
    pub(crate) fn keyword(self) -> &'static str {
        match self {
            Self::Contains => "CONTAINS",
            Self::IContains => "ICONTAINS",
            Self::StartsWith => "STARTS_WITH",
            Self::EndsWith => "ENDS_WITH",
            Self::IEquals => "IEQUALS",
            Self::Fuzzy => "FUZZY",
        }
    }

    pub(crate) const ALL: [Self; 6] = [
        Self::Contains,
        Self::IContains,
        Self::StartsWith,
        Self::EndsWith,
        Self::IEquals,
        Self::Fuzzy,
    ];
}

#[derive(Debug, Clone)]
pub(crate) enum TextMatcher {
    Regex(Regex),
    Fuzzy(String),
}

impl PartialEq for TextMatcher {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Regex(a), Self::Regex(b)) => a.as_str() == b.as_str(),
            (Self::Fuzzy(a), Self::Fuzzy(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for TextMatcher {}

impl TextMatcher {
    pub(crate) fn regex(pattern: &str) -> Result<Self, PipelineError> {
        Ok(Self::Regex(Regex::new(pattern)?))
    }

    pub(crate) fn literal(mode: TextMatch, pattern: &str) -> Result<Self, PipelineError> {
        if mode == TextMatch::Fuzzy {
            if pattern.trim().is_empty() {
                return Err(PipelineError::Parse("FUZZY requires nonempty text".into()));
            }
            return Ok(Self::Fuzzy(pattern.to_lowercase()));
        }
        let escaped = escape(pattern);
        let pattern = match mode {
            TextMatch::StartsWith => format!("\\A{escaped}"),
            TextMatch::EndsWith => format!("{escaped}\\z"),
            TextMatch::IEquals => format!("(?i)\\A{escaped}\\z"),
            TextMatch::IContains => format!("(?i){escaped}"),
            _ => escaped,
        };
        Ok(Self::Regex(RegexBuilder::new(&pattern).build()?))
    }

    pub(crate) fn is_match(&self, text: &str) -> bool {
        match self {
            Self::Regex(regex) => regex.is_match(text),
            Self::Fuzzy(query) => {
                let text = text.to_lowercase();
                if text.contains(query) {
                    return true;
                }
                let length = query.chars().count();
                if length < 3 {
                    return false;
                }
                let edits = if length < 6 { 1 } else { 2 };
                std::iter::once(text.as_str())
                    .chain(
                        text.split(|ch: char| !ch.is_alphanumeric())
                            .filter(|word| !word.is_empty()),
                    )
                    .any(|candidate| {
                        candidate.chars().count().abs_diff(length) <= edits
                            && damerau_levenshtein(candidate, query) <= edits
                    })
            }
        }
    }
}
