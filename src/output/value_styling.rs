use std::ops::Range;

use anstyle::Style;
use hubuum_theme::{paint_style, Theme};
use serde_json::{from_str, value::RawValue};

/// Styles refer to the original text so wrapping never changes classification
/// or requires interpreting ANSI escape sequences as printable characters.
pub(super) struct StyledValue<'a> {
    text: &'a str,
    base: Style,
    spans: Vec<StyledSpan>,
    list: bool,
}

struct StyledSpan {
    range: Range<usize>,
    style: Style,
}

impl<'a> StyledValue<'a> {
    pub(super) fn plain(text: &'a str, style: Style) -> Self {
        Self {
            text,
            base: style,
            spans: Vec::new(),
            list: false,
        }
    }

    pub(super) fn new(text: &'a str, theme: Option<&Theme>) -> Self {
        let Some(theme) = theme else {
            return Self::plain(text, Style::new());
        };
        if !text.trim_start().starts_with('[') || from_str::<&RawValue>(text).is_err() {
            return Self::plain(text, theme.value_style(text));
        }
        let mut value = Self {
            text,
            base: Style::new(),
            spans: Vec::new(),
            list: true,
        };
        let bytes = text.as_bytes();
        let mut offset = 0;
        // JSON has already been validated. Locate scalar lexemes without
        // reserializing them, including strings with escaped quotes or Unicode.
        while offset < bytes.len() {
            match bytes[offset] {
                b'"' => {
                    let start = offset;
                    offset += 1;
                    while bytes[offset] != b'"' {
                        offset += if bytes[offset] == b'\\' { 2 } else { 1 };
                    }
                    offset += 1;
                    // Object keys inside a list are structural, not field values.
                    if text[offset..].trim_start().starts_with(':') {
                        continue;
                    }
                    if let Ok(decoded) = from_str::<String>(&text[start..offset]) {
                        value.spans.push(StyledSpan {
                            range: start + 1..offset - 1,
                            style: theme.value_style(&decoded),
                        });
                    }
                }
                b'[' | b']' | b'{' | b'}' | b':' | b',' | b' ' | b'\n' | b'\r' | b'\t' => {
                    offset += 1
                }
                _ => {
                    let start = offset;
                    while offset < bytes.len()
                        && !matches!(
                            bytes[offset],
                            b']' | b'}' | b',' | b' ' | b'\n' | b'\r' | b'\t'
                        )
                    {
                        offset += 1;
                    }
                    value.spans.push(StyledSpan {
                        range: start..offset,
                        style: theme.value_style(&text[start..offset]),
                    });
                }
            }
        }
        value
    }

    pub(super) fn text(&self) -> &'a str {
        self.text
    }

    pub(super) fn is_list(&self) -> bool {
        self.list
    }

    pub(super) fn paint(&self) -> String {
        self.paint_range(0..self.text.len(), Style::new())
    }

    /// Reset each line before the renderer adds continuation indentation.
    pub(super) fn painted_lines(&self) -> impl Iterator<Item = String> + '_ {
        let mut offset = 0;
        self.text.split('\n').map(move |line| {
            let range = offset..offset + line.len();
            offset += line.len() + 1;
            self.paint_range(range, Style::new())
        })
    }

    pub(super) fn paint_range(&self, range: Range<usize>, band: Style) -> String {
        let mut rendered = String::new();
        let mut offset = range.start;
        let first = self
            .spans
            .partition_point(|span| span.range.end <= range.start);
        for span in self.spans[first..]
            .iter()
            .take_while(|span| span.range.start < range.end)
        {
            let start = span.range.start.max(range.start);
            let end = span.range.end.min(range.end);
            if start >= end {
                continue;
            }
            rendered.push_str(&paint_style(
                over_band(self.base, band),
                &self.text[offset..start],
            ));
            // Colors precede effects so a table's intensity reset before each
            // background cannot cancel this span's bold or dim attributes.
            let style = over_band(span.style, band);
            let colors = style.effects(Default::default());
            let effects = Style::new().effects(style.get_effects());
            rendered.push_str(&format!(
                "{}{}{}{}",
                colors.render(),
                effects.render(),
                &self.text[start..end],
                style.render_reset()
            ));
            offset = end;
        }
        rendered.push_str(&paint_style(
            over_band(self.base, band),
            &self.text[offset..range.end],
        ));
        rendered
    }

    pub(super) fn paint_padded(&self, range: Range<usize>, width: usize, band: Style) -> String {
        let padding = width.saturating_sub(self.text[range.clone()].chars().count());
        if self.spans.is_empty() {
            return paint_style(
                over_band(self.base, band),
                format!("{}{}", &self.text[range], " ".repeat(padding)),
            );
        }
        let mut rendered = self.paint_range(range, band);
        rendered.push_str(&paint_style(
            over_band(self.base, band),
            " ".repeat(padding),
        ));
        rendered
    }
}

pub(super) fn over_band(style: Style, band: Style) -> Style {
    style
        .fg_color(style.get_fg_color().or(band.get_fg_color()))
        .bg_color(style.get_bg_color().or(band.get_bg_color()))
        .effects(style.get_effects() | band.get_effects())
}

#[cfg(test)]
mod tests {
    use anstyle::AnsiColor;
    use hubuum_theme::{resolve_theme, ColorSpec, RoleStyle, ThemeRole};

    use crate::output::tests::without_ansi;

    use super::*;

    #[test]
    fn each_network_list_item_is_colored_without_styling_json_punctuation() {
        let theme = resolve_theme("hubuum-dark", None).unwrap();
        for (first, second) in [
            ("129.240.222.179", "192.168.50.193"),
            ("129.240.223.255", "192.168.50.255"),
            ("255.255.254.0", "255.255.255.0"),
            ("129.240.222.0", "192.168.50.0"),
        ] {
            let source = format!(r#"["{first}","{second}"]"#);
            let styled = StyledValue::new(&source, Some(&theme));
            assert_eq!(
                styled.paint(),
                format!("[\"\x1b[32m{first}\x1b[0m\",\"\x1b[32m{second}\x1b[0m\"]")
            );
        }
    }

    #[test]
    fn nested_mixed_lists_preserve_lexemes_and_do_not_classify_object_keys() {
        let theme = resolve_theme("hubuum-dark", None).unwrap();
        let source = r#" [ "192\u002e168.0.1", true, null, 4.20e+2, {"192.0.2.1": ["::1", "2020-07-30T11:46:02"]}, "escaped \"quote\"", "å🙂", "" ] "#;
        let styled = StyledValue::new(source, Some(&theme));
        let lexemes = styled
            .spans
            .iter()
            .map(|span| &source[span.range.clone()])
            .collect::<Vec<_>>();
        assert_eq!(
            lexemes,
            [
                r"192\u002e168.0.1",
                "true",
                "null",
                "4.20e+2",
                "::1",
                "2020-07-30T11:46:02",
                r#"escaped \"quote\""#,
                "å🙂",
                ""
            ]
        );
        assert_eq!(without_ansi(&styled.paint()), source);
        assert_eq!(styled.spans[0].style, theme.value_style("192.168.0.1"));
        assert_eq!(
            styled.spans[5].style,
            theme.value_style("2020-07-30T11:46:02")
        );
    }

    #[test]
    fn invalid_lists_and_uncolored_output_remain_plain() {
        let theme = resolve_theme("hubuum-dark", None).unwrap();
        for source in [
            "[192.168.0.1]",
            r#"["192.168.0.1",]"#,
            r#"["unterminated]"#,
            r#"["::1"] trailing"#,
            "[INFO] 192.168.0.1",
        ] {
            let styled = StyledValue::new(source, Some(&theme));
            assert!(!styled.is_list(), "{source}");
            assert_eq!(styled.paint(), source);
        }
        assert_eq!(
            StyledValue::new(r#"["::1",null]"#, None).paint(),
            r#"["::1",null]"#
        );
    }

    #[test]
    fn list_items_honor_category_overrides_and_wrapped_ranges() {
        let mut theme = resolve_theme("hubuum-dark", None).unwrap();
        theme.roles.insert(
            ThemeRole::Ipv4,
            RoleStyle::new(Some(ColorSpec::ansi(AnsiColor::Blue)), None, false),
        );
        let source = r#"["129.240.222.179"]"#;
        let styled = StyledValue::new(source, Some(&theme));
        assert_eq!(
            styled.paint_range(0..8, Style::new()),
            "[\"\x1b[34m129.24\x1b[0m"
        );
        assert_eq!(
            styled.paint_range(8..source.len(), Style::new()),
            "\x1b[34m0.222.179\x1b[0m\"]"
        );
    }

    #[test]
    fn list_items_honor_disabled_classifiers() {
        let theme = resolve_theme("hubuum-dark", None).unwrap();
        let mut definition = serde_json::to_value(theme).unwrap();
        definition["classifiers"]["disabled"] = serde_json::json!(["ipv4", "date"]);
        let theme: Theme = serde_json::from_value(definition).unwrap();
        let source = r#"["129.240.222.179", "2020-07-30T11:46:02"]"#;
        assert_eq!(StyledValue::new(source, Some(&theme)).paint(), source);
    }
}
