use super::*;
#[test]
fn validates_json_root_fields() {
    let specs = [FilterFieldSpec::new(
        "data",
        "data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root()];
    let clauses = vec![filter_clause(
        "data.owner.email",
        FilterOperator::IContains { is_negated: false },
        "alice",
    )];

    let validated = validate_filter_clauses(&clauses, &specs).expect("json root should validate");
    assert_eq!(validated[0].json_path(), vec!["owner", "email"]);
}

#[test]
fn rejects_unsupported_operators() {
    let specs = [FilterFieldSpec::new(
        "validate_schema",
        "validate_schema",
        FilterOperatorProfile::Boolean,
        FilterValueProfile::Boolean,
    )];
    let err = validate_filter_clauses(
        &[filter_clause(
            "validate_schema",
            FilterOperator::Contains { is_negated: false },
            "true",
        )],
        &specs,
    )
    .expect_err("invalid boolean operator should fail");

    assert!(err.to_string().contains("not supported"));
}

#[test]
fn rejects_unknown_sort_fields() {
    let specs = [SortFieldSpec::new("name", "name")];
    let err = validate_sort_clauses(
        &[SortClause {
            field: "nme".to_string(),
            direction: SortDirectionArg::Asc,
        }],
        &specs,
    )
    .expect_err("unknown sort field should fail");

    assert!(err.to_string().contains("Unknown sort field"));
    assert!(err.to_string().contains("Did you mean 'name'?"));
}

#[test]
fn rejects_unknown_filter_fields_with_suggestion() {
    let specs = [FilterFieldSpec::new(
        "name",
        "name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    )];
    let err = validate_filter_clauses(
        &[filter_clause(
            "nme",
            FilterOperator::IContains { is_negated: false },
            "alice",
        )],
        &specs,
    )
    .expect_err("unknown filter field should fail");

    assert!(err.to_string().contains("Unknown filter field"));
    assert!(err.to_string().contains("Did you mean 'name'?"));
}

#[test]
fn rejects_unknown_operator_and_direction_with_suggestions() {
    let operator_err = parse_where_clause("name contans alice").expect_err("operator should fail");
    assert!(operator_err.to_string().contains("Unknown filter operator"));
    assert!(operator_err
        .to_string()
        .contains("Did you mean 'contains'?"));

    let sort_err = parse_sort_clause("name dsc").expect_err("direction should fail");
    assert!(sort_err.to_string().contains("Unknown sort direction"));
    assert!(sort_err.to_string().contains("Did you mean 'desc'?"));
}

#[test]
fn resolves_json_root_specs_for_completion() {
    let specs = [FilterFieldSpec::new(
        "data",
        "data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root()];

    let (spec, json_path) =
        resolve_filter_field_spec(&specs, "data.owner.email").expect("json field should match");
    assert_eq!(spec.public_name(), "data");
    assert_eq!(json_path, vec!["owner", "email"]);
}

#[test]
fn completion_operator_profiles_expose_expected_words() {
    assert!(completion_operators(FilterOperatorProfile::String).contains(&"icontains"));
    assert!(completion_operators(FilterOperatorProfile::Boolean).contains(&"equals"));
    assert!(!completion_operators(FilterOperatorProfile::Boolean).contains(&"regex"));
}

#[test]
fn name_resolution_does_not_change_the_validated_domain_value() {
    let specs = [FilterFieldSpec::new(
        "class",
        "hubuum_class_id",
        FilterOperatorProfile::EqualityOnly,
        FilterValueProfile::String,
    )
    .resolver(FilterValueResolver::ClassNameToId)];
    let clauses =
        validate_filter_clauses(&[parse_where_clause("class equals Hosts").unwrap()], &specs)
            .unwrap();
    let filter = clauses[0]
        .resolve_value(|resolver, name| {
            assert_eq!(resolver, FilterValueResolver::ClassNameToId);
            assert_eq!(name, "Hosts");
            Ok::<_, std::convert::Infallible>("42".to_string())
        })
        .unwrap();
    assert_eq!(filter.key, "hubuum_class_id");
    assert_eq!(filter.value, "42");
    assert_eq!(clauses[0].value(), "Hosts");
}

#[test]
fn json_path_encoding_preserves_the_value() {
    let specs = [FilterFieldSpec::new(
        "data",
        "json_data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root()];
    let clauses = validate_filter_clauses(
        &[parse_where_clause("data.owner.email equals a=b@example.com").unwrap()],
        &specs,
    )
    .unwrap();
    let filter = clauses[0]
        .resolve_value(|_, value| Ok::<_, std::convert::Infallible>(value.to_string()))
        .unwrap();
    assert_eq!(filter.key, "json_data");
    assert_eq!(filter.value, "owner,email=a=b@example.com");
}
