use crate::tokenizer::CommandTokenizer;

pub(crate) fn shell_escape(token: &str) -> String {
    if token.is_empty() {
        return "''".to_string();
    }

    if token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '='))
    {
        token.to_string()
    } else {
        format!("'{}'", token.replace('\'', "'\\''"))
    }
}

pub(crate) fn rebuild_with_replaced_options<'a>(
    tokens: &CommandTokenizer,
    remove_options: &[&str],
    appended: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> String {
    let mut rebuilt = Vec::new();
    let mut skip_next = false;

    for token in tokens.raw_tokens() {
        if skip_next {
            skip_next = false;
            continue;
        }

        if remove_options.iter().any(|option| token == option) {
            skip_next = true;
            continue;
        }

        if remove_options
            .iter()
            .any(|option| token.starts_with(&format!("{option}=")))
        {
            continue;
        }

        rebuilt.push(shell_escape(token));
    }

    for (option, value) in appended {
        if let Some(value) = value {
            rebuilt.push(option.to_string());
            rebuilt.push(shell_escape(value));
        }
    }

    rebuilt.join(" ")
}

#[cfg(test)]
mod tests {
    use super::rebuild_with_replaced_options;
    use crate::tokenizer::CommandTokenizer;

    #[test]
    fn rebuild_replaces_separate_and_inline_options() {
        let tokens = CommandTokenizer::new(
            "class list --cursor old --limit 2 --cursor=older",
            "list",
            &[],
        )
        .expect("tokenizer");

        assert_eq!(
            rebuild_with_replaced_options(
                &tokens,
                &["--cursor"],
                [("--cursor", Some("new value"))],
            ),
            "class list --limit 2 --cursor 'new value'"
        );
    }
}
