use std::time::{Duration, UNIX_EPOCH};

use hubuum_filter::{
    pipeline_help, split_pipeline, statement_help, MissingSelectorPolicy, OutputEnvelope, Pipeline,
    PipelineSettings,
};
use serde_json::{json, Value};

fn apply(source: &str, rows: Value) -> Value {
    Pipeline::parse(source)
        .unwrap()
        .apply(OutputEnvelope::rows(
            rows.as_array().unwrap().clone(),
            Vec::new(),
        ))
        .unwrap()
        .into_value()
}

#[test]
fn integer_precision_is_shared_by_predicates_sort_distinct_and_aggregates() {
    let rows = json!([{"n": 9007199254740993_u64}, {"n": 9007199254740992_u64}]);
    assert_eq!(
        apply("S n", rows.clone()),
        json!([{"n":9007199254740992_u64},{"n":9007199254740993_u64}])
    );
    assert_eq!(
        apply("S n AS num", rows.clone()),
        apply("S n", rows.clone())
    );
    assert_eq!(apply("D n AS num", rows.clone()), rows);
    for source in [
        "F WHERE n > 9007199254740992",
        "F WHERE n AS num > 9007199254740992",
        "F n>9007199254740992",
    ] {
        assert_eq!(
            apply(source, rows.clone()),
            json!([{"n":9007199254740993_u64}]),
            "{source}"
        );
    }
    assert_eq!(
        apply("A GLOBAL sum(n) AS sum, min(n) AS min, max(n) AS max", rows),
        json!([{"sum":18014398509481985_u64,"min":9007199254740992_u64,"max":9007199254740993_u64}])
    );
    assert_eq!(
        apply(
            "D n AS num",
            json!([{"n":1},{"n":"1"},{"n":1.0},{"n":-0.0},{"n":0}])
        ),
        json!([{"n":1},{"n":-0.0}])
    );
    assert_eq!(
        apply(
            "S n AS num",
            json!([{"n":18446744073709551616.0},{"n":u64::MAX},{"n":-1.5},{"n":-1},{"n":i64::MIN}])
        ),
        json!([{"n":i64::MIN},{"n":-1.5},{"n":-1},{"n":u64::MAX},{"n":18446744073709551616.0}])
    );
    assert_eq!(
        apply("A GLOBAL sum(n) AS total", json!([{"n":1.0},{"n":2}])),
        json!([{"total":3.0}])
    );
}

#[test]
fn relative_times_share_an_injected_clock_and_refresh_on_reuse() {
    let pipeline = Pipeline::parse("F WHERE at AS datetime >= now-1d AND at AS datetime <= now | F WHERE at AS datetime == now").unwrap();
    let rows = OutputEnvelope::rows(vec![json!({"at":"2026-01-01T00:00:00Z"})], Vec::new());
    let settings =
        PipelineSettings::new().with_evaluation_time(UNIX_EPOCH + Duration::from_secs(1767225600));
    assert_eq!(
        pipeline
            .apply_with_settings(rows.clone(), &settings)
            .unwrap()
            .value(),
        &json!([{"at":"2026-01-01T00:00:00Z"}])
    );
    let later = settings.with_evaluation_time(UNIX_EPOCH + Duration::from_secs(1767225601));
    assert!(pipeline
        .apply_with_settings(rows, &later)
        .unwrap()
        .is_empty());
    for source in [
        "F WHERE at >= now",
        "F WHERE at AS datetime >= now-1w",
        "F WHERE at AS datetime >= now-999999999999999999999d",
        "F WHERE at AS datetime >= now-💥",
        "F WHERE at AS datetime IN [now]",
    ] {
        assert!(Pipeline::parse(source).is_err(), "{source}");
    }
}

#[test]
fn literal_and_fuzzy_matching_are_explicit_and_keep_missing_semantics() {
    let rows = json!([{"name":"a.b"},{"name":"axb"},{"name":"SERVER"},{"name":"servers"},{"name":"several"},{}]);
    assert_eq!(
        apply("F WHERE name CONTAINS 'a.b'", rows.clone()),
        json!([{"name":"a.b"}])
    );
    assert_eq!(
        apply("F WHERE name ~ 'a.b'", rows.clone()),
        json!([{"name":"a.b"},{"name":"axb"}])
    );
    assert_eq!(
        apply("F WHERE name IEQUALS 'server'", rows.clone()),
        json!([{"name":"SERVER"}])
    );
    assert_eq!(
        apply("F WHERE name FUZZY 'servre'", rows.clone()),
        json!([{"name":"SERVER"},{"name":"servers"}])
    );
    assert!(!apply("F WHERE name NOT FUZZY 'server'", rows)
        .as_array()
        .unwrap()
        .contains(&json!({})));
    assert_eq!(
        apply("F WHERE name ICONTAINS 'Ø'", json!([{"name":"søren"}])),
        json!([{"name":"søren"}])
    );
    assert_eq!(
        apply(
            "F WHERE name STARTS_WITH 'a.' AND name ENDS_WITH '.b'",
            json!([{"name":"a.b"}])
        ),
        json!([{"name":"a.b"}])
    );
    assert!(Pipeline::parse("F WHERE name FUZZY ''").is_err());
    assert!(Pipeline::parse("F WHERE name AS num CONTAINS '1'").is_err());
    assert_eq!(
        apply(
            "F WHERE tags[] NOT CONTAINS 'x'",
            json!([{"tags":["a","b"]},{"tags":["x","b"]},{"tags":[]}])
        ),
        json!([{"tags":["a","b"]}])
    );
}

#[test]
fn structural_edits_resolve_original_addresses_and_preserve_member_identity() {
    let input = json!([{"items":["a","b","c","d"]}]);
    for source in [
        "P !items[0] !items[1]",
        "P !items[1] !items[0]",
        "P !items[:2] !items[0]",
    ] {
        assert_eq!(
            apply(source, input.clone()),
            json!([{"items":["c","d"]}]),
            "{source}"
        );
    }
    assert_eq!(
        apply("P items[1:3] AS Tail, !items[1]", input.clone()),
        json!([{"Tail":"c"}])
    );
    assert_eq!(
        apply("P items AS List, !List[0], !List[1]", input),
        json!([{"List":["c","d"]}])
    );
    assert_eq!(
        apply(
            "P !items[0] !items[1].secret",
            json!([{"items":[{"id":0},{"id":1,"secret":true},{"id":2,"secret":true}]}])
        ),
        json!([{"items":[{"id":1},{"id":2,"secret":true}]}])
    );
    let rows = json!([{"host":"one","items":[{"id":1},{"id":2,"name":null},{"name":"third"}]}]);
    assert_eq!(
        apply(
            "P ROWS host AS Host, items[].id AS ID, items[].name AS Name",
            rows
        ),
        json!([{"Host":"one","ID":1},{"Host":"one","ID":2,"Name":null},{"Host":"one","Name":"third"}])
    );
    assert_eq!(
        apply(
            "P ROWS items[].id AS ID",
            json!([{"items":[{"id":1},{"id":1}]}])
        ),
        json!([{"ID":1},{"ID":1}])
    );
    assert_eq!(
        apply(
            "P ROWS name AS Name, items[].id AS ID",
            json!([{"name":"one","items":[]}])
        ),
        json!([])
    );
}

#[test]
fn unroll_destinations_prevent_silent_parent_overwrites() {
    let rows = json!([{"name":"parent","members":[{"name":"child","id":1}]}]);
    let input = OutputEnvelope::rows(rows.as_array().unwrap().clone(), Vec::new());
    let error = Pipeline::parse("U members")
        .unwrap()
        .apply(input)
        .unwrap_err()
        .to_string();
    assert!(error.contains("AS item"), "{error}");
    assert_eq!(
        apply("U members AS member | P name member.name", rows),
        json!([{"name":"parent","member.name":"child"}])
    );
    let input = OutputEnvelope::rows(vec![json!({"items":[1],"member":2})], Vec::new());
    assert!(Pipeline::parse("U items AS member")
        .unwrap()
        .apply(input)
        .is_err());
}

#[test]
fn group_membership_counts_rows_once_but_selected_counts_keep_occurrences() {
    assert_eq!(
        apply(
            "G tags[] AS Tag | A count(tags[]) AS Occurrences | C",
            json!([{"tags":["ops","ops"]}])
        ),
        json!([{"Tag":"ops","Occurrences":2,"count":1}])
    );
    assert_eq!(
        apply(
            "G tags[] AS Tag teams[] AS Team | C",
            json!([{"tags":["a","a"],"teams":[1,1,2]}])
        ),
        json!([{"Tag":"a","Team":1,"count":1},{"Tag":"a","Team":2,"count":1}])
    );
}

#[test]
fn strict_selector_diagnostics_are_opt_in_and_allow_intentional_missing_fields() {
    let rows = json!([{"name":"one"}]);
    assert_eq!(apply("S naem", rows.clone()), rows);
    let input = OutputEnvelope::rows(rows.as_array().unwrap().clone(), Vec::new());
    for source in [
        "CHECK STRICT | S naem",
        "CHECK STRICT | F WHERE naem == 'one'",
        "CHECK STRICT | P naem",
        "CHECK STRICT | G naem",
    ] {
        let error = Pipeline::parse(source)
            .unwrap()
            .apply(input.clone())
            .unwrap_err()
            .to_string();
        assert!(error.contains("did you mean 'name'"), "{source}: {error}");
    }
    assert_eq!(
        apply(
            "CHECK STRICT | F WHERE missing IS MISSING OR missing == 'one'",
            rows.clone()
        ),
        rows
    );
    assert_eq!(
        apply("CHECK STRICT | CHECK ALLOW | P missing", rows),
        json!([{"missing":null}])
    );
    assert_eq!(apply("CHECK STRICT | S naem", json!([])), json!([]));
    assert_eq!(
        apply(
            "CHECK STRICT | F WHERE items[].name == 'a'",
            json!([{"items":[]}])
        ),
        json!([])
    );
    let settings =
        PipelineSettings::new().with_missing_selector_policy(MissingSelectorPolicy::Error);
    assert!(Pipeline::parse("S naem")
        .unwrap()
        .apply_with_settings(input, &settings)
        .is_err());
}

#[test]
fn shared_scanner_preserves_typed_escapes_and_shell_quoted_literals() {
    let source = r#"F WHERE name == 'it\'s|here' AND note CONTAINS 'a\\b' | P name"#;
    assert_eq!(
        apply(source, json!([{"name":"it's|here","note":"a\\b"}])),
        json!([{"name":"it's|here"}])
    );
    let (_, stages) = split_pipeline(&format!("command | {source}")).unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(
        apply(r#"P name AS 'a\' | P 'a\'"#, json!([{"name":"one"}])),
        json!([{"a\\":"one"}])
    );
    for source in [
        "F WHERE name == 'unfinished",
        "P name AS 'unfinished",
        "P name\\",
    ] {
        assert!(Pipeline::parse(source)
            .unwrap_err()
            .to_string()
            .contains("byte"));
    }
}

#[test]
fn help_explains_the_statement_and_does_not_evaluate_data_stages() {
    let pipeline = Pipeline::parse("F WHERE n AS num > 2 | S n desc | HELP CURRENT").unwrap();
    let help = pipeline_help(pipeline.stages()).unwrap();
    assert!(help.contains("1. F WHERE n AS num > 2"));
    assert!(help.contains("2. S n"));
    assert!(help.contains("Accepts:"));
    let input = OutputEnvelope::rows(vec![json!({"n":"invalid"})], Vec::new());
    assert!(pipeline.apply(input).is_ok());
    for topic in [
        "F",
        "P",
        "S",
        "D",
        "A",
        "U",
        "CHECK",
        "selectors",
        "shapes",
        "JQ",
    ] {
        assert!(statement_help(&format!("HELP {topic}")).unwrap().is_some());
    }
    assert!(statement_help("object delete --id 12 | H")
        .unwrap()
        .unwrap()
        .contains("does not execute"));
    assert!(Pipeline::parse("H | C").is_err());
    assert!(Pipeline::parse("H nonexistent").is_err());
    assert!(statement_help("help pipe").unwrap().is_none());
    let impossible = Pipeline::parse("G rack | G rack | HELP CURRENT").unwrap();
    assert!(pipeline_help(impossible.stages())
        .unwrap()
        .contains("No supported shape transition"));
    assert!(statement_help("object delete | H | C").is_err());
    assert!(statement_help("H | C").is_err());
    assert!(Pipeline::parse("P ROWS").is_err());
    assert_eq!(
        apply("P\tROWS\titems[] AS Item", json!([{"items":[1,2]}])),
        json!([{"Item":1},{"Item":2}])
    );
    let aggregates = Pipeline::parse("G rack | A sum(size) AS Total | H CURRENT").unwrap();
    let help = pipeline_help(aggregates.stages()).unwrap();
    assert!(help.contains("sum(size) AS Total"));
    assert!(!help.contains("Selector {"));
}
