use hubuum_filter::{
    Comparison, Predicate as ParsedPredicate, PredicateExpr, PredicateOperator, PredicateTest,
    TypedLiteral,
};
use serde_json::Value;

use super::{require, Expression, Predicate, SearchError};

pub(super) fn parse(source: &str) -> Result<Expression, SearchError> {
    check_complexity(source)?;
    let parsed = ParsedPredicate::parse(source).map_err(|error| SearchError(error.to_string()))?;
    lower(parsed.expression(), 1)
}

fn check_complexity(source: &str) -> Result<(), SearchError> {
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_usize;
    let mut unquoted = String::new();
    for ch in source.chars() {
        if let Some(delimiter) = quote {
            if ch == delimiter && !escaped {
                quote = None;
            }
            escaped = ch == '\\' && !escaped;
            unquoted.push(' ');
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            unquoted.push(' ');
        } else {
            if ch == '(' {
                depth += 1;
            }
            if ch == ')' {
                depth = depth.saturating_sub(1);
            }
            require(depth <= 8, "search predicates are limited to depth 8")?;
            unquoted.push(ch);
        }
    }
    require(
        unquoted
            .split(|ch: char| !ch.is_ascii_alphabetic())
            .filter(|word| {
                ["AND", "OR", "NOT"]
                    .iter()
                    .any(|keyword| word.eq_ignore_ascii_case(keyword))
            })
            .count()
            <= 64,
        "search predicate exceeds 64 boolean operators",
    )
}

fn lower(expression: &PredicateExpr, depth: usize) -> Result<Expression, SearchError> {
    require(depth <= 8, "search predicates are limited to depth 8")?;
    match expression {
        PredicateExpr::And(a, b) => Ok(Expression::And {
            args: vec![lower(a, depth + 1)?, lower(b, depth + 1)?],
        }),
        PredicateExpr::Or(a, b) => Ok(Expression::Or {
            args: vec![lower(a, depth + 1)?, lower(b, depth + 1)?],
        }),
        PredicateExpr::Not(arg) => Ok(Expression::Not {
            arg: Box::new(lower(arg, depth + 1)?),
        }),
        PredicateExpr::Test(test) => lower_test(test),
    }
}

fn lower_test(test: &PredicateTest) -> Result<Expression, SearchError> {
    require(
        test.cast().is_none(),
        "server search does not support AS casts; use typed literals or a local pipeline",
    )?;
    let selector = test.selector().as_str();
    let (field, path) = if let Some(path) = selector.strip_prefix("data.") {
        ("json_data", Some(path.to_string()))
    } else if let Some((root, path)) = selector.split_once('.') {
        (root, Some(path.to_string()))
    } else {
        (selector, None)
    };
    let (operator, value, negated) = match test.operator() {
        PredicateOperator::Compare { comparison, literal } => {
            let operator = match comparison {
                Comparison::Equal | Comparison::NotEqual => "equals",
                Comparison::Less => "lt", Comparison::LessOrEqual => "lte",
                Comparison::Greater => "gt", Comparison::GreaterOrEqual => "gte",
            };
            (operator, Some(literal_value(literal)?), *comparison == Comparison::NotEqual)
        }
        PredicateOperator::Regex { pattern, negated } => ("regex", Some(Value::String(pattern.clone())), *negated),
        PredicateOperator::In { literals, negated } => ("in", Some(Value::Array(literals.iter().map(literal_value).collect::<Result<_, _>>()?)), *negated),
        PredicateOperator::IsNull { negated } => ("is_null", None, *negated),
        PredicateOperator::IsMissing { .. } => return Err(SearchError("server search does not distinguish missing paths from null; use IS NULL or a local pipeline".into())),
    };
    let expression = Expression::Field {
        predicate: Predicate {
            field: field.to_string(),
            path,
            operator: operator.into(),
            value,
        },
    };
    Ok(if negated {
        Expression::Not {
            arg: Box::new(expression),
        }
    } else {
        expression
    })
}

fn literal_value(literal: &TypedLiteral) -> Result<Value, SearchError> {
    match literal {
        TypedLiteral::String(value) => Ok(Value::String(value.clone())),
        TypedLiteral::Number(value) => Ok(Value::Number(value.clone())),
        TypedLiteral::Boolean(value) => Ok(Value::Bool(*value)),
        TypedLiteral::Null => Err(SearchError(
            "use IS NULL or IS NOT NULL instead of comparing with null".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::check_complexity;
    use crate::{ResourceKind, SearchRequest};
    use serde_json::{json, to_value};

    #[test]
    fn rejects_large_negation_chains_before_recursive_parsing_in_every_case() {
        for keyword in ["NOT", "not", "nOt"] {
            let source = format!("{}name == \"a\"", format!("{keyword} ").repeat(10_000));
            assert!(source.len() < crate::MAX_REQUEST_BYTES);
            // Fail this assertion before invoking the recursive parser if the guard regresses.
            assert!(check_complexity(&source).is_err(), "{keyword}");
            let error = SearchRequest::new(ResourceKind::Object)
                .with_predicate(&source)
                .unwrap_err();
            assert!(error.to_string().contains("64 boolean operators"));
        }
    }

    #[test]
    fn counts_conjunctions_and_disjunctions_case_insensitively() {
        for keyword in ["AND", "and", "aNd", "OR", "or", "oR"] {
            let source = format!(
                "{}name == \"a\"",
                format!("name == \"a\" {keyword} ").repeat(65)
            );
            let error = SearchRequest::new(ResourceKind::Object)
                .with_predicate(&source)
                .unwrap_err();
            assert!(
                error.to_string().contains("64 boolean operators"),
                "{keyword}: {error}"
            );
        }
    }

    #[test]
    fn accepts_mixed_case_expressions_and_ignores_quoted_boolean_words() {
        let source = r#"not name == "and OR nOt" aNd (name == "srv" oR name == "host")"#;
        let request = SearchRequest::new(ResourceKind::Object)
            .with_predicate(source)
            .unwrap();
        assert_eq!(to_value(request).unwrap()["filter"]["op"], "and");

        let literal = "and OR nOt ".repeat(1_000);
        let request = SearchRequest::new(ResourceKind::Object)
            .with_predicate(&format!("name == {}", json!(literal)))
            .unwrap();
        assert_eq!(
            to_value(request).unwrap()["filter"]["predicate"]["value"],
            literal
        );
    }

    #[test]
    fn compiles_boolean_precedence_typed_values_and_json_paths() {
        let request = SearchRequest::new(ResourceKind::Object)
            .in_class("Hosts")
            .unwrap()
            .with_predicate(
                r#"data.cpu.cores >= 8 AND (name == "srv-01" OR NOT data.retired == true)"#,
            )
            .unwrap();
        assert_eq!(
            to_value(request).unwrap()["filter"],
            json!({"op":"and","args":[
                {"op":"field","predicate":{"field":"json_data","path":"cpu.cores","operator":"gte","value":8}},
                {"op":"or","args":[
                    {"op":"field","predicate":{"field":"name","operator":"equals","value":"srv-01"}},
                    {"op":"not","arg":{"op":"field","predicate":{"field":"json_data","path":"retired","operator":"equals","value":true}}}
                ]}
            ]})
        );
    }

    #[test]
    fn rejects_local_only_semantics_and_wrong_target_fields() {
        for predicate in [
            "data.x AS num > 1",
            "data.x IS MISSING",
            "data.x == null",
            "data.x[] == 1",
            "email == 'alice'",
        ] {
            assert!(
                SearchRequest::new(ResourceKind::Object)
                    .with_predicate(predicate)
                    .is_err(),
                "{predicate}"
            );
        }
    }

    #[test]
    fn maps_negation_and_membership() {
        let request = SearchRequest::new(ResourceKind::User)
            .with_predicate(r#"name IN ["alice", "bob"] AND email IS NOT NULL"#)
            .unwrap();
        let filter = to_value(request).unwrap()["filter"].clone();
        assert_eq!(
            filter["args"][0]["predicate"]["value"],
            json!(["alice", "bob"])
        );
        assert_eq!(filter["args"][1]["op"], "not");
    }
}
