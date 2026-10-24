use hubuum_client::{
    ApiError, ClassId, FilterOperator, JsonPath, QueryFilter, RelatedObjectFilter,
    StructuredClassSelector,
};

use crate::{
    parse_where_clause, validate_filter_clause, FilterFieldSpec, FilterOperatorProfile,
    FilterValueProfile, QueryError,
};

/// Validated related-object groups for class object lists. SDK types are the
/// intentional integration surface, as with the ordinary list-query filters.
#[derive(Debug, Clone, Default)]
pub struct RelatedObjectFilters {
    groups: Vec<Group>,
}

#[derive(Debug, Clone)]
struct Group {
    filter: RelatedObjectFilter,
    predicates: Vec<QueryFilter>,
    depth_set: bool,
}

impl RelatedObjectFilters {
    /// Add `ALIAS=CLASS`, preserving the exact class name, including numeric names.
    pub fn add_class(&mut self, definition: &str) -> Result<(), QueryError> {
        let (alias, name) = assignment(definition)?;
        self.add(alias, StructuredClassSelector::Name { name: name.into() })
    }

    /// Add `ALIAS=ID` with a positive class ID.
    pub fn add_class_id(&mut self, definition: &str) -> Result<(), QueryError> {
        let (alias, id) = assignment(definition)?;
        let id = id.parse::<ClassId>().map_err(invalid_sdk)?;
        self.add(alias, StructuredClassSelector::Id { id })
    }

    fn add(&mut self, alias: &str, class: StructuredClassSelector) -> Result<(), QueryError> {
        let filter = RelatedObjectFilter::new(alias, class).map_err(invalid_sdk)?;
        if self
            .groups
            .iter()
            .any(|group| group.filter.alias() == alias)
        {
            return Err(QueryError::InvalidOption(format!(
                "Duplicate related alias '{alias}'"
            )));
        }
        if self.groups.len() == 4 {
            return Err(QueryError::InvalidOption(
                "At most four related groups are allowed".into(),
            ));
        }
        self.groups.push(Group {
            filter,
            predicates: Vec::new(),
            depth_set: false,
        });
        Ok(())
    }

    /// Set `ALIAS=DEPTH` once per group, from 1 through 10 (default 1).
    pub fn set_depth(&mut self, definition: &str) -> Result<(), QueryError> {
        let (alias, depth) = assignment(definition)?;
        let group = self.group_mut(alias)?;
        if group.depth_set {
            return Err(QueryError::InvalidOption(format!(
                "Duplicate related depth for '{alias}'"
            )));
        }
        let depth = depth.parse::<u8>().map_err(|_| {
            QueryError::InvalidOption("Related depth must be between 1 and 10".into())
        })?;
        group.filter = group.filter.clone().max_depth(depth).map_err(invalid_sdk)?;
        group.depth_set = true;
        Ok(())
    }

    /// Add `ALIAS.FIELD OP VALUE`. All predicates in a group match one target.
    pub fn add_where(&mut self, clause: &str) -> Result<(), QueryError> {
        let mut clause = parse_where_clause(clause)?;
        let (alias, field) = clause.field.split_once('.').ok_or_else(|| {
            QueryError::InvalidOption("Related filters require ALIAS.FIELD OP VALUE".into())
        })?;
        let group = self.group_mut(alias)?;
        clause.field = field.to_owned();
        let validated = validate_filter_clause(&clause, RELATED_OBJECT_FILTER_FIELDS)?;
        if validated.spec().is_json_root() {
            JsonPath::new(validated.json_path()).map_err(invalid_sdk)?;
        }
        let predicate = validated.resolve_value::<QueryError>(|_, value| Ok(value.into()))?;
        group.predicates.push(predicate);
        Ok(())
    }

    fn group_mut(&mut self, alias: &str) -> Result<&mut Group, QueryError> {
        self.groups
            .iter_mut()
            .find(|group| group.filter.alias() == alias)
            .ok_or_else(|| {
                QueryError::InvalidOption(format!(
                    "Unknown related alias '{alias}'; declare it with --related or --related-id"
                ))
            })
    }

    pub fn sdk_filters(&self) -> impl Iterator<Item = RelatedObjectFilter> + '_ {
        self.groups.iter().map(|group| group.filter.clone())
    }

    /// The SDK's computed-object cursor lacks `related()`. Use the same wire
    /// parameters there; transport tests verify parity with the typed helper.
    pub fn computed_filters(&self) -> Vec<QueryFilter> {
        let mut filters = Vec::new();
        for group in &self.groups {
            let prefix = format!("related.{}", group.filter.alias());
            let (field, value) = match group.filter.class() {
                StructuredClassSelector::Name { name } => ("name", name.clone()),
                StructuredClassSelector::Id { id } => ("id", id.to_string()),
            };
            filters.push(QueryFilter::raw(format!("{prefix}.class.{field}"), value));
            filters.push(QueryFilter::filter(
                format!("{prefix}.depth"),
                FilterOperator::Lte { is_negated: false },
                group.filter.depth().to_string(),
            ));
        }
        filters.extend(self.predicate_filters());
        filters
    }

    /// Dynamic predicates validated with the list-query grammar. Add these after
    /// the typed SDK groups, which reserve their aliases when added to a query.
    pub fn predicate_filters(&self) -> Vec<QueryFilter> {
        self.groups
            .iter()
            .flat_map(|group| {
                group.predicates.iter().cloned().map(|mut predicate| {
                    predicate.key =
                        format!("related.{}.object.{}", group.filter.alias(), predicate.key);
                    predicate
                })
            })
            .collect()
    }
}

fn assignment(value: &str) -> Result<(&str, &str), QueryError> {
    value
        .split_once('=')
        .ok_or_else(|| QueryError::InvalidOption("Related options require ALIAS=VALUE".into()))
}

fn invalid_sdk(error: ApiError) -> QueryError {
    QueryError::InvalidOption(error.to_string())
}

pub const RELATED_OBJECT_FILTER_FIELDS: &[FilterFieldSpec] = &[
    FilterFieldSpec::new(
        "id",
        "id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "name",
        "name",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "description",
        "description",
        FilterOperatorProfile::String,
        FilterValueProfile::String,
    ),
    FilterFieldSpec::new(
        "collection_id",
        "collection_id",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "revision",
        "revision",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::Integer,
    ),
    FilterFieldSpec::new(
        "created_at",
        "created_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "updated_at",
        "updated_at",
        FilterOperatorProfile::NumericOrDate,
        FilterValueProfile::DateTime,
    ),
    FilterFieldSpec::new(
        "data",
        "json_data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root(),
    FilterFieldSpec::new(
        "json_data",
        "json_data",
        FilterOperatorProfile::Any,
        FilterValueProfile::Any,
    )
    .json_root(),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_preserve_exact_names_and_typed_class_ids() {
        let mut filters = RelatedObjectFilters::default();
        filters.add_class("room=Server Rooms / ø &=1").unwrap();
        filters.add_class("numeric=123").unwrap();
        filters.add_class_id("rack=42").unwrap();
        filters.set_depth("room=2").unwrap();
        filters.add_where("room.name equals B 701").unwrap();
        filters.add_where("room.data.floor gte 3").unwrap();
        let wire = filters.computed_filters();
        assert!(wire
            .iter()
            .any(|f| f.key == "related.room.class.name" && f.value == "Server Rooms / ø &=1"));
        assert!(wire
            .iter()
            .any(|f| f.key == "related.numeric.class.name" && f.value == "123"));
        assert!(wire
            .iter()
            .any(|f| f.key == "related.rack.class.id" && f.value == "42"));
        assert!(wire
            .iter()
            .any(|f| f.key == "related.room.object.name" && f.value == "B 701"));
        assert!(wire
            .iter()
            .any(|f| f.key == "related.room.object.json_data" && f.value == "floor=3"));
        assert_eq!(filters.sdk_filters().next().unwrap().depth(), 2);
    }

    #[test]
    fn definitions_reject_invalid_or_duplicate_aliases_and_overflow() {
        let mut filters = RelatedObjectFilters::default();
        for definition in ["room", "=Rooms", "9room=Rooms", "bad.alias=Rooms", "room= "] {
            assert!(filters.add_class(definition).is_err(), "{definition}");
        }
        for definition in ["room=0", "room=-1", "room=abc"] {
            assert!(filters.add_class_id(definition).is_err(), "{definition}");
        }
        filters.add_class("room=Rooms").unwrap();
        assert!(filters.add_class_id("room=1").is_err());
        for alias in ["a", "b", "c"] {
            filters.add_class(&format!("{alias}=Rooms")).unwrap();
        }
        assert!(filters.add_class("fifth=Rooms").is_err());
        assert_eq!(filters.sdk_filters().count(), 4);
    }

    #[test]
    fn depths_are_bounded_and_cannot_be_repeated_or_orphaned() {
        let mut filters = RelatedObjectFilters::default();
        assert!(filters.set_depth("room=2").is_err());
        filters.add_class("room=Rooms").unwrap();
        for value in ["0", "11", "-1", "256", "many"] {
            assert!(filters.set_depth(&format!("room={value}")).is_err());
            assert_eq!(filters.sdk_filters().next().unwrap().depth(), 1);
        }
        filters.set_depth("room=10").unwrap();
        assert!(filters.set_depth("room=2").is_err());
        assert_eq!(filters.sdk_filters().next().unwrap().depth(), 10);
    }

    #[test]
    fn predicates_reject_unknown_fields_operators_types_and_paths() {
        let mut filters = RelatedObjectFilters::default();
        filters.add_class("room=Rooms").unwrap();
        for clause in [
            "name equals B701",
            "unknown.name equals B701",
            "room.class equals Rooms",
            "room.S:floor equals 3",
            "room.name gt B701",
            "room.id equals abc",
            "room.created_at equals yesterday",
            "room.data equals x",
            "room.data..floor equals x",
            "room.json_data...floor equals x",
            "room.data.a..b equals x",
            "room.data.a,b equals x",
            "room.data.a=b equals x",
            "room.data.a[0] equals x",
        ] {
            assert!(filters.add_where(clause).is_err(), "{clause}");
        }
        assert!(filters.predicate_filters().is_empty());
        filters.add_where("room.name not_equals B701").unwrap();
        filters.add_where("room.data.floor between 2,4").unwrap();
        assert_eq!(filters.predicate_filters().len(), 2);
    }
}
