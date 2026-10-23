mod views;

use cli_command_derive::CommandArgs;
use hubuum_client::{ClassId, HistoryId, HubuumDateTime, ObjectId};
use serde::{Deserialize, Serialize};

use super::builder::{catalog_command, CommandDocs};
use super::{
    normalize_server_page_size, option_or_pos, render_json_record, render_list_page, CliCommand,
    PageSelection,
};
use crate::autocomplete::{classes, objects_from_class};
use crate::catalog::{CommandCatalogBuilder, CommandEffects};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::services::{
    parse_timestamp, AppServices, HistoryInput, HistoryListInput, ResourceScope,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    views::register_commands(builder);
    builder
        .add_command(
            &["history"],
            catalog_command(
                "class",
                ClassHistory::default(),
                CommandDocs {
                    about: Some("List class temporal history"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["history"],
            catalog_command(
                "object",
                ObjectHistory::default(),
                CommandDocs {
                    about: Some("List object temporal history"),
                    ..CommandDocs::default()
                },
            ),
        )
        .add_command(
            &["history"],
            catalog_command(
                "show",
                HistoryShow::default(),
                CommandDocs {
                    about: Some("Show one class or object history record"),
                    long_about: Some(
                        "Shows a complete class history record, or an object history record when --name or --object-id is supplied. Select exactly one version with --id or --at. ID lookup uses the resource-scoped history endpoint. Use --class-id and --object-id to read retained history without resolving a live name.",
                    ),
                    examples: Some(
                        "--class Hosts --name host.example.org --id 1498\n--class Hosts --name host.example.org --at 2026-07-21T20:17:03Z\n--class Hosts --id 42",
                    ),
                },
            ),
        );
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct HistoryShow {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    pub class: Option<String>,
    #[option(
        long = "class-id",
        help = "Positive class ID; mutually exclusive with --class"
    )]
    pub class_id: Option<ClassId>,
    #[option(
        long = "name",
        help = "Object name; omit for class history",
        autocomplete = "objects_from_class"
    )]
    pub name: Option<String>,
    #[option(
        long = "object-id",
        help = "Positive object ID; mutually exclusive with --name"
    )]
    pub object_id: Option<ObjectId>,
    #[option(long = "id", help = "History record ID")]
    pub id: Option<HistoryId>,
    #[option(long = "at", help = "As-of RFC3339 timestamp")]
    pub at: Option<String>,
}

struct HistoryArgs {
    limit: Option<usize>,
    sort: Option<String>,
    cursor: Option<String>,
    at: Option<String>,
    include_total: bool,
    page_selection: PageSelection,
}

impl HistoryArgs {
    fn into_input(self) -> Result<HistoryInput, AppError> {
        if let Some(at) = self.at {
            if self.limit.is_some()
                || self.sort.is_some()
                || self.cursor.is_some()
                || matches!(self.page_selection, PageSelection::All)
            {
                return Err(AppError::InvalidOption(
                    "--at cannot be combined with --limit, --sort, --cursor, or --all".into(),
                ));
            }
            return Ok(HistoryInput::At {
                at: parse_timestamp(&at)?,
                include_total: self.include_total,
            });
        }
        Ok(HistoryInput::List(
            HistoryListInput::default()
                .limit(self.limit.map(TryInto::try_into).transpose()?)
                .sort(self.sort.as_deref())?
                .cursor(self.cursor)
                .include_total(self.include_total)
                .page_selection(self.page_selection),
        ))
    }
}

enum HistorySelector {
    Id(HistoryId),
    At(HubuumDateTime),
}

impl CliCommand for HistoryShow {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.id = option_or_pos(query.id, tokens, 0, "id")?;
        let scope = ResourceScope::class(query.class, query.class_id)?
            .with_object(query.name, query.object_id)?;

        let record = match history_selector(query.id, query.at)? {
            HistorySelector::Id(id) => services.gateway().history_record_by_id(scope, id)?,
            HistorySelector::At(at) => services.gateway().history_record_at(scope, at)?,
        };

        render_json_record(tokens, &record)
    }
}

fn history_selector(
    id: Option<HistoryId>,
    at: Option<String>,
) -> Result<HistorySelector, AppError> {
    match (id, at) {
        (Some(id), None) => Ok(HistorySelector::Id(id)),
        (None, Some(at)) => Ok(HistorySelector::At(parse_timestamp(&at)?)),
        (Some(_), Some(_)) => Err(AppError::InvalidOption(
            "--id and --at are mutually exclusive".to_string(),
        )),
        (None, None) => Err(AppError::MissingOptions(
            vec!["one of id or at".to_string()],
        )),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ClassHistory {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    pub class: Option<String>,
    #[option(
        long = "class-id",
        help = "Positive class ID; mutually exclusive with --class"
    )]
    pub class_id: Option<ClassId>,
    #[option(long = "at", help = "As-of RFC3339 timestamp")]
    pub at: Option<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -history_id")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
    #[option(
        long = "all",
        help = "Fetch and buffer all result pages before applying pipelines",
        flag = "true"
    )]
    pub all: Option<bool>,
}

impl CliCommand for ClassHistory {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        query.class = query
            .class
            .or_else(|| tokens.get_positionals().first().cloned());
        let history = services.gateway().history(
            ResourceScope::class(query.class, query.class_id)?,
            HistoryArgs {
                limit: normalize_server_page_size(query.limit)?,
                sort: query.sort,
                cursor: query.cursor,
                at: query.at,
                include_total: query.include_total.unwrap_or(false),
                page_selection: PageSelection::from_all(query.all.unwrap_or(false)),
            }
            .into_input()?,
        )?;
        render_list_page(tokens, &history)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, CommandArgs, Default)]
pub struct ObjectHistory {
    #[option(long = "class", help = "Class name", autocomplete = "classes")]
    pub class: Option<String>,
    #[option(
        long = "class-id",
        help = "Positive class ID; mutually exclusive with --class"
    )]
    pub class_id: Option<ClassId>,
    #[option(
        long = "name",
        help = "Object name",
        autocomplete = "objects_from_class"
    )]
    pub name: Option<String>,
    #[option(
        long = "object-id",
        help = "Positive object ID; mutually exclusive with --name"
    )]
    pub object_id: Option<ObjectId>,
    #[option(long = "at", help = "As-of RFC3339 timestamp")]
    pub at: Option<String>,
    #[option(long = "limit", help = "Page size (server maximum: 250)")]
    pub limit: Option<usize>,
    #[option(long = "sort", help = "Sort expression, e.g. -history_id")]
    pub sort: Option<String>,
    #[option(long = "cursor", help = "Cursor for the next page")]
    pub cursor: Option<String>,
    #[option(
        long = "include-total",
        help = "Request the exact matching count",
        flag = "true"
    )]
    pub include_total: Option<bool>,
    #[option(
        long = "all",
        help = "Fetch and buffer all result pages before applying pipelines",
        flag = "true"
    )]
    pub all: Option<bool>,
}

impl CliCommand for ObjectHistory {
    const REAUTHENTICATION_RETRY: ReauthenticationRetry = ReauthenticationRetry::Safe;
    const EFFECTS: CommandEffects = CommandEffects::ReadOnly;

    fn execute(&self, services: &AppServices, tokens: &CommandTokenizer) -> Result<(), AppError> {
        let mut query = Self::parse_tokens(tokens)?;
        let positionals = tokens.get_positionals();
        let positional_class = query.class.is_none() && query.class_id.is_none();
        if positional_class {
            query.class = positionals.first().cloned();
        }
        if query.name.is_none() && query.object_id.is_none() {
            query.name = positionals.get(usize::from(positional_class)).cloned();
        }
        let history = services.gateway().history(
            ResourceScope::class(query.class, query.class_id)?
                .with_object(query.name, query.object_id)?
                .require_object()?
                .into(),
            HistoryArgs {
                limit: normalize_server_page_size(query.limit)?,
                sort: query.sort,
                cursor: query.cursor,
                at: query.at,
                include_total: query.include_total.unwrap_or(false),
                page_selection: PageSelection::from_all(query.all.unwrap_or(false)),
            }
            .into_input()?,
        )?;
        render_list_page(tokens, &history)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn history_modes_validate_before_resource_resolution() {
        use super::HistoryArgs;
        use crate::list_query::PageSelection;
        let args = || HistoryArgs {
            limit: None,
            sort: None,
            cursor: None,
            at: Some("2026-07-21T20:17:03Z".into()),
            include_total: true,
            page_selection: PageSelection::Single,
        };
        assert!(matches!(
            args().into_input().unwrap(),
            crate::services::HistoryInput::At {
                include_total: true,
                ..
            }
        ));
        let mut invalid = args();
        invalid.at = Some("yesterday".into());
        assert!(invalid.into_input().is_err());
        let mut invalid = args();
        invalid.limit = Some(10);
        assert!(invalid.into_input().is_err());
        let mut invalid = args();
        invalid.page_selection = PageSelection::All;
        assert!(invalid.into_input().is_err());
        let mut invalid = args();
        invalid.cursor = Some("next".into());
        assert!(invalid.into_input().is_err());
        let mut invalid = args();
        invalid.sort = Some("-history_id".into());
        assert!(invalid.into_input().is_err());
    }

    use super::{history_selector, HistorySelector};

    #[test]
    fn history_show_accepts_exactly_one_selector() {
        assert!(matches!(
            history_selector(Some(1498.into()), None).expect("id should be accepted"),
            HistorySelector::Id(id) if id.get() == 1498
        ));
        assert!(matches!(
            history_selector(None, Some("2026-07-21T20:17:03Z".to_string()))
                .expect("timestamp should be accepted"),
            HistorySelector::At(at) if at == crate::services::parse_timestamp("2026-07-21T20:17:03Z").unwrap()
        ));
        assert!(
            history_selector(Some(1498.into()), Some("2026-07-21T20:17:03Z".to_string())).is_err()
        );
        assert!(history_selector(None, None).is_err());
    }
}
