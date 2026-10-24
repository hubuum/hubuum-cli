use crate::scanner::split_unquoted;
use crate::OutputShape;
use crate::{
    split_pipeline, MissingSelectorPolicy, NullOrder, PipeStage, Pipeline, PipelineError, SortCast,
    SortDirection, SortReduction,
};

/// Inspect CLI statement help before dispatch, including commands with effects.
/// Ordinary `help` remains the host application's command; standalone `H` and
/// `HELP` and terminal pipe help are owned by this language.
pub fn statement_help(source: &str) -> Result<Option<String>, PipelineError> {
    let segments = split_unquoted(source, '|')?;
    let standalone = matches!(source.split_whitespace().next(), Some("H" | "h" | "HELP"));
    let terminal = segments.len() > 1
        && segments
            .iter()
            .skip(1)
            .filter_map(|stage| stage.split_whitespace().next())
            .any(|verb| verb.eq_ignore_ascii_case("H") || verb.eq_ignore_ascii_case("HELP"));
    if !standalone && !terminal {
        return Ok(None);
    }
    let stages = if standalone {
        Pipeline::parse(source)?.into_stages()
    } else {
        split_pipeline(source)?.1
    };
    Ok(pipeline_help(&stages).map(|help| format!("Current statement: {source}\n\n{help}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpRequest {
    topic: Option<String>,
}

impl HelpRequest {
    pub fn new(topic: Option<&str>) -> Result<Self, PipelineError> {
        if let Some(topic) = topic {
            if topic_help(topic).is_none() {
                return Err(PipelineError::Parse(format!(
                    "Unknown pipeline help topic '{topic}'; use H for available options"
                )));
            }
        }
        Ok(Self {
            topic: topic.map(str::to_string),
        })
    }
    pub fn topic(&self) -> Option<&str> {
        self.topic.as_deref()
    }
}

/// Help is a terminal inspection request, so hosts can handle it before dispatch.
pub fn pipeline_help(stages: &[PipeStage]) -> Option<String> {
    let PipeStage::Help(request) = stages.last()? else {
        return None;
    };
    let mut output = match request.topic() {
        Some(topic) => topic_help(topic).expect("validated help topic").to_string(),
        None => {
            let mut text = String::from("Pipeline options (H / HELP <verb|topic|CURRENT>):\n");
            for verb in verb_summaries() {
                text.push_str(&format!("  {:<20} {}\n", verb.names, verb.summary));
            }
            text.push_str("\nTopics: ");
            text.push_str(
                &help_topics()
                    .iter()
                    .map(|topic| topic.name)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            text.push_str("\nTyped predicates: F WHERE ...; use H F for operators, casts, time and fuzzy matching.\nPipelines transform fetched semantic data before rendering; use --all on paginated commands when needed.\n");
            text
        }
    };
    output.push_str("\n\nCurrent pipeline:\n");
    if stages.len() == 1 {
        output.push_str("  No data stages.\n");
    }
    let mut shapes = vec![
        OutputShape::Empty,
        OutputShape::Lines,
        OutputShape::Rows,
        OutputShape::Detail,
        OutputShape::Message,
        OutputShape::Values,
        OutputShape::Groups,
    ];
    for (index, stage) in stages[..stages.len() - 1].iter().enumerate() {
        let detail = match stage {
            PipeStage::TypedFilter(predicate) | PipeStage::TypedReject(predicate) => {
                predicate.source().to_string()
            }
            PipeStage::Grep(pattern)
            | PipeStage::Reject(pattern)
            | PipeStage::ValueSearch(pattern)
            | PipeStage::KeySearch(pattern)
            | PipeStage::Jq(pattern) => pattern.clone(),
            PipeStage::Columns(terms) | PipeStage::ProjectRows(terms) => terms
                .iter()
                .map(|term| {
                    format!(
                        "{}{}{}",
                        if term.is_drop() { "!" } else { "" },
                        term.selector(),
                        term.alias()
                            .map(|alias| format!(" AS {alias}"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
            PipeStage::Value(selector) | PipeStage::Unroll(selector) => selector.to_string(),
            PipeStage::UnrollInto(spec) => format!("{} AS {}", spec.selector(), spec.destination()),
            PipeStage::SortColumns(spec) => spec
                .keys()
                .iter()
                .map(|key| {
                    let cast = if key.cast() == SortCast::Auto {
                        String::new()
                    } else {
                        format!(" AS {}", key.cast())
                    };
                    format!(
                        "{} {}{cast} USING {} NULLS {}",
                        key.selector(),
                        match key.direction() {
                            SortDirection::Ascending => "asc",
                            SortDirection::Descending => "desc",
                        },
                        match key.reduction() {
                            SortReduction::First => "first",
                            SortReduction::Min => "min",
                            SortReduction::Max => "max",
                        },
                        match key.null_order() {
                            NullOrder::First => "FIRST",
                            NullOrder::Last => "LAST",
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
            PipeStage::Group(keys) => keys
                .iter()
                .map(|key| format!("{} AS {}", key.selector(), key.alias()))
                .collect::<Vec<_>>()
                .join(", "),
            PipeStage::Aggregate(request) => request
                .specs()
                .iter()
                .map(|spec| format!("{} AS {}", spec.function(), spec.alias()))
                .collect::<Vec<_>>()
                .join(", "),
            PipeStage::Distinct(spec) => spec
                .keys()
                .iter()
                .map(|key| {
                    format!(
                        "{}{}",
                        key.selector(),
                        key.cast()
                            .map(|cast| format!(" AS {cast}"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
            PipeStage::Head { count, offset } => format!("{count} offset {offset}"),
            PipeStage::Tail(count) => count.to_string(),
            PipeStage::SelectorPolicy(policy) => match policy {
                MissingSelectorPolicy::Allow => "ALLOW",
                MissingSelectorPolicy::Error => "STRICT",
            }
            .into(),
            PipeStage::Truthy(selector) => selector
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            PipeStage::SortLines { descending } => {
                if *descending { "line desc" } else { "line asc" }.into()
            }
            PipeStage::Count | PipeStage::CollapseGroups | PipeStage::Help(_) => String::new(),
        };
        output.push_str(&format!(
            "  {}. {} {}\n     Accepts: {}\n",
            index + 1,
            stage.name(),
            detail,
            stage
                .accepted_input_shapes()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
        let mut resulting = Vec::new();
        for input in &shapes {
            if let Ok(outputs) = stage.resulting_shapes(*input) {
                for output in outputs {
                    if !resulting.contains(output) {
                        resulting.push(*output);
                    }
                }
            }
        }
        if resulting.is_empty() {
            output.push_str("     No supported shape transition from the preceding stages.\n");
        } else {
            output.push_str(&format!(
                "     Possible output: {}\n",
                resulting
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        shapes = resulting;
    }
    output.push_str("\nHelp does not execute the command or its pipeline. Input shapes, values, and selector availability are checked when executing.");
    Some(output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerbSummary {
    names: &'static str,
    topic: &'static str,
    summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpTopic {
    name: &'static str,
    title: &'static str,
    summary: &'static str,
}

impl VerbSummary {
    pub fn names(&self) -> &'static str {
        self.names
    }
    pub fn topic(&self) -> &'static str {
        self.topic
    }
    pub fn summary(&self) -> &'static str {
        self.summary
    }
}
impl HelpTopic {
    pub fn name(&self) -> &'static str {
        self.name
    }
    pub fn title(&self) -> &'static str {
        self.title
    }
    pub fn summary(&self) -> &'static str {
        self.summary
    }
}

pub fn help_topics() -> &'static [HelpTopic] {
    &[
        HelpTopic {
            name: "help",
            title: "Pipeline Help",
            summary: "H, HELP, and CURRENT without executing the command.",
        },
        HelpTopic {
            name: "check",
            title: "Selector Diagnostics",
            summary: "Opt-in CHECK STRICT and CHECK ALLOW.",
        },
        HelpTopic {
            name: "search",
            title: "Search And Filter",
            summary: "Bare search, F, V, K, reject, and ?.",
        },
        HelpTopic {
            name: "project",
            title: "Projection And Values",
            summary: "P, P ROWS, VALUE, and VAL.",
        },
        HelpTopic {
            name: "sort",
            title: "Sorting",
            summary: "S and sort, including typed casts.",
        },
        HelpTopic {
            name: "distinct",
            title: "Stable Distinct",
            summary: "D and distinct over visible values or selector tuples.",
        },
        HelpTopic {
            name: "limit",
            title: "Limits And Counts",
            summary: "L, head, tail, and C.",
        },
        HelpTopic {
            name: "group",
            title: "Grouping And Aggregates",
            summary: "G, A, grouped C, Z, and U.",
        },
        HelpTopic {
            name: "selectors",
            title: "Selectors",
            summary: "Dotted paths, indexes, [*], [], negative indexes, and slices.",
        },
        HelpTopic {
            name: "shapes",
            title: "Shape Contracts",
            summary: "Accepted input and resulting output shapes for every stage.",
        },
        HelpTopic {
            name: "jq",
            title: "JQ",
            summary: "JQ transforms over the current semantic payload.",
        },
        HelpTopic {
            name: "redirects",
            title: "Redirects",
            summary: ">, >>, and each:<template>.",
        },
    ]
}

pub fn verb_summaries() -> &'static [VerbSummary] {
    &[
        VerbSummary {
            names: "H, HELP",
            topic: "help",
            summary: "Explain options and the current pipeline without executing it.",
        },
        VerbSummary {
            names: "CHECK",
            topic: "check",
            summary: "Choose STRICT or ALLOW missing-selector diagnostics.",
        },
        VerbSummary {
            names: "bare, F, grep",
            topic: "search",
            summary: "Search rows by keys and values, or filter a specific field.",
        },
        VerbSummary {
            names: "reject",
            topic: "search",
            summary: "Remove rows matching a pattern or typed predicate.",
        },
        VerbSummary {
            names: "?",
            topic: "search",
            summary: "Keep truthy rows or test a selector for a non-empty value.",
        },
        VerbSummary {
            names: "V",
            topic: "search",
            summary: "Search values only.",
        },
        VerbSummary {
            names: "K",
            topic: "search",
            summary: "Search keys only and project matched keys.",
        },
        VerbSummary {
            names: "P, columns",
            topic: "project",
            summary: "Choose or alias fields to keep; !field drops fields.",
        },
        VerbSummary {
            names: "P ROWS",
            topic: "project",
            summary: "Project rows aligned by original array member addresses.",
        },
        VerbSummary {
            names: "VALUE, VAL",
            topic: "project",
            summary: "Extract one selector as a value list.",
        },
        VerbSummary {
            names: "S, sort",
            topic: "sort",
            summary: "Sort rows by a field or line value.",
        },
        VerbSummary {
            names: "D, distinct",
            topic: "distinct",
            summary: "Keep the first row for each visible value or selector tuple.",
        },
        VerbSummary {
            names: "L, head, tail",
            topic: "limit",
            summary: "Keep a subset of rows.",
        },
        VerbSummary {
            names: "C, count",
            topic: "limit",
            summary: "Count rows or grouped rows.",
        },
        VerbSummary {
            names: "G",
            topic: "group",
            summary: "Group rows by one or more selectors.",
        },
        VerbSummary {
            names: "A",
            topic: "group",
            summary: "Aggregate grouped rows or a complete collection.",
        },
        VerbSummary {
            names: "Z",
            topic: "group",
            summary: "Collapse groups into summary rows.",
        },
        VerbSummary {
            names: "U",
            topic: "group",
            summary: "Unroll arrays; AS item keeps each member in a named field.",
        },
        VerbSummary {
            names: "JQ",
            topic: "jq",
            summary: "Apply a jq-compatible expression.",
        },
    ]
}

pub fn topic_help(topic: &str) -> Option<&'static str> {
    match topic.to_ascii_lowercase().as_str() {
        "f" | "grep" | "v" | "k" | "reject" | "?" | "where" | "search" | "fuzzy" | "contains" | "icontains" | "iequals" | "starts_with" | "ends_with" | "datetime" => Some(
            "Search stages:\n  | pattern - keep rows where key paths or visible or hidden values match a regex.\n  | F <pattern> - same as bare search, useful when the pattern looks like syntax.\n  | F <field> <regex> - keep rows where one selector matches a regex.\n  | F <field><op><value> - compact legacy =, !=, ~, >, >=, <, or <= predicate.\n  | F WHERE <predicate> - typed boolean predicate with NOT, AND, OR, and parentheses.\n  | reject WHERE <predicate> - negate one complete typed predicate.\n  | V <pattern> - search scalar values only, ignoring key names.\n  | K <pattern> - search key paths only and project matching keys.\n  | reject <pattern> - remove rows matching a broad pattern.\n  | reject <field> <regex> - remove rows where one selector matches.\n  | ? [field] - keep truthy rows, or rows where a selector has a non-empty value.\n\nTyped tests support =, ==, !=, <, <=, >, >=, ~, !~, MATCHES, IN, IS NULL, IS MISSING, CONTAINS, ICONTAINS, STARTS_WITH, ENDS_WITH, IEQUALS, and FUZZY. Prefix IN and text operators with NOT to negate. CONTAINS/STARTS_WITH/ENDS_WITH are case-sensitive literal matches; ICONTAINS/IEQUALS use Unicode simple case folding. FUZZY is opt-in: lowercase substring or whole-word edit matching, one edit for 3-5 characters and two for 6+; shorter queries require a substring. Cast with AS str|num|bool|ip|datetime|version|natural. JSON numbers, booleans, and null are unquoted; strings are quoted. Relative operands now and now-Ns/m/h/d require AS datetime and share one clock sample per application; datetime strings require explicit RFC3339 offsets. Integer comparisons and AS num preserve exact integer identity.\n\nExamples:\n  object list --class Hosts | F os_version 26\n  object list --class Hosts | F data.cpu.cores>=8\n  object list --class Hosts | F WHERE data.cpu.cores AS num >= 8 AND state IN [\"ready\", \"running\"]\n  object list --class Hosts | reject WHERE owner IS MISSING OR disabled == true\n  object list --class Hosts | V 129.240\n  object list --class Hosts | K ipv4\n  object list --class Hosts | ? data.network.interfaces[]",
        ),
        "p" | "columns" | "value" | "val" | "project" => Some(
            "Projection stages:\n  | P ROWS <selector> [AS alias], ... - emit rows aligned by original array member address; repeat static fields.\n  | P <field> [field...] - keep selected fields as table columns.\n  | P <field> AS <name>, <field> AS <name> - rename selected values as top-level output fields; commas are required when any alias is used.\n  | P <field> !<field> - keep selected fields and drop excluded fields; drop terms cannot use AS.\n  | VALUE <path> - extract selector matches as a value list.\n  | VAL <path> - short alias for VALUE.\n\nDrop terms resolve together before array indexes change. P ROWS keeps explicit output names and leaves unmatched member fields absent; it does not zip unrelated arrays or form a Cartesian product. Ordinary P aliases retain selector cardinality: no match becomes null, one match stays scalar, and multiple matches become an array. Empty, duplicate, and conflicting grouped output names fail before rendering.\n\nExamples:\n  object list --class Hosts | P Name os_version data.network.interfaces[*].ipv4\n  object list --class Hosts | P Name AS Host, data.network.interfaces[].ipv4 AS Addresses\n  object list --class Hosts | P Name data !data.secrets\n  object list --class Hosts | VALUE data.network.interfaces[*].ipv4",
        ),
        "s" | "sort" => Some(
            "Sort stages:\n  | S <field> - sort rows ascending by one selector.\n  | S !<field> - sort rows descending by one selector.\n  | S <key>, <key> - stable lexicographic multi-key sort.\n\nEach key accepts asc|desc, AS str|num|bool|ip|datetime|version|natural, USING first|min|max, and NULLS FIRST|LAST in that order. Missing and null keys default to last for both directions. USING first is the fanout compatibility default. AS ip strictly validates std::net::IpAddr values, orders IPv4 before IPv6 ascending, and keeps mapped IPv6 in the IPv6 family.\n\nExamples:\n  object list --class Hosts | S os_version\n  object list --class Hosts | S state asc, updated_at desc AS datetime, Name AS natural\n  object list --class Hosts | S data.network.interfaces[].ipv4 AS ip USING min, Name\n  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts | S Hosts desc AS num",
        ),
        "d" | "distinct" => Some(
            "Stable distinct stages:\n  | D - keep the first occurrence of each complete visible JSON value.\n  | D <selector>, <selector> - keep the first occurrence of each ordered selector tuple.\n  | distinct <selector> AS <cast> - use a strict typed identity.\n\nKeys accept AS str|num|bool|ip|datetime|version|natural. Fanout selectors contribute their complete ordered sequence; missing differs from JSON null. Object field order does not affect whole-value equality. Empty, lines, rows, values, and groups are supported; grouped equality sees summaries and never merges members. Detail and message output are rejected.\n\nExamples:\n  object list --class Hosts | D\n  object list --class Hosts | D owner, os_version\n  object list --class Hosts | D data.network.interfaces[].ipv4 AS ip\n  object list --class Hosts | G rack | A count AS Hosts | D Hosts AS num",
        ),
        "l" | "head" | "tail" | "c" | "count" | "limit" => Some(
            "Limit and count stages:\n  | L [count] [offset] - keep a window of rows from the current result.\n  | head [count] [offset] - readable alias for L.\n  | tail [count] - keep rows from the end of the current result.\n  | C - replace rows with a count.\n  | count - readable alias for C.\n\nExamples:\n  object list --class Hosts | L 10\n  object list --class Hosts | L 10 20\n  object list --class Hosts | os_version contains 26 | C",
        ),
        "g" | "a" | "u" | "z" | "group" => Some(
            "Grouping and aggregation stages:\n  | G <field> [AS alias] - group rows by one selector, optionally naming the output column.\n  | A <aggregate> [AS alias] - add one aggregate to each group.\n  | A GLOBAL <aggregate> [AS alias], ... - consume Empty, Rows, or Values and return one row.\n  | Z - collapse groups to one summary row per group.\n  | U <array-field> [AS item] - unroll arrays. AS places each member in a new field without promoting its fields; existing destinations are errors. The unaliased form rejects parent/member name collisions; use AS to resolve them.\n\nAggregates are count, count(field), count_distinct(field), sum(field), avg(field), min(field), max(field), first(field), and last(field). count(field) counts every non-null fanout match. count_distinct(field) excludes missing and null and uses the same JSON equality as D. first(field) and last(field) skip missing and null and use current row and selector order; sort before G or A GLOBAL for reproducible boundaries. Global output names must be unique. Use --all when a paginated command must aggregate every matching result.\n\nRepeated selected keys put an input row into each group once. After G, filters, truthiness, key/value search, projection, sorting, and unroll operate on visible group and aggregate aliases. Filters retain or remove whole groups; hidden member rows never change. Grouped C emits each summary with its member count. Use Z before A GLOBAL when aggregating visible group summaries.\n\nExamples:\n  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts\n  object list --class Hosts --all | A GLOBAL count AS Hosts, count(data.owner) AS Owned, count_distinct(os_version) AS Versions\n  task events <task-id> --all | S created_at asc AS datetime | A GLOBAL first(created_at) AS First, last(created_at) AS Last\n  audit list --all | S occurred_at asc AS datetime | G action | A first(occurred_at) AS First | A last(occurred_at) AS Last | Z\n  object list --class Hosts | G data.network.interfaces[*].ipv4 AS IPv4 | C\n  object list --class Hosts | U data.network.interfaces | P Name ipv4 mac",
        ),
        "check" => Some("CHECK STRICT rejects selectors absent from every record of a nonempty input and lists available paths with typo suggestions. CHECK ALLOW restores the default permissive policy. Checks apply to explicit fields, not broad searches; IS MISSING and selectors guarded by an IS MISSING test are exempt. Empty input and empty fanout/slice selections never cause a missing-selector error.\nExample: command | CHECK STRICT | S name"),
        "h" | "help" | "current" => Some("H / HELP [verb|topic|CURRENT] explains the language and the current pipeline without running its command. Place help last. Use H F, H P, H S, H CHECK, or H shapes for focused help. CURRENT describes parsed stages and their accepted shapes; data-dependent checks still happen at execution."),
        "selectors" => Some(
            "Selectors:\n  name                         field lookup\n  data.owner                   dotted path\n  data.network.interfaces[0]   array index\n  data.network.interfaces[-1]  negative index\n  data.network.interfaces[*]   fan out array\n  data.network.interfaces[]    fan out array\n  data.network.interfaces[:2]  slice",
        ),
        "shapes" => Some(
            "Shape contracts:\n  Shapes are Empty, Lines, Rows, Detail, Message, Values, and Groups.\n  Legacy F, V, and reject accept every shape. Typed F WHERE and reject WHERE require structured shapes. K and ? accept every shape except Lines.\n  L, head, tail, and whole-line S accept Empty, Lines, Rows, Values, and Groups.\n  Field S and U accept Empty, Rows, Values, and Groups.\n  P and P ROWS accept Empty, Rows, Detail, Message, and Groups.\n  D accepts Empty, Lines, Rows, Values, and Groups; selector-based D excludes Lines.\n  G accepts Empty, Rows, Detail, Message, and Values; grouped A and Z require Groups. A GLOBAL accepts Empty, Rows, and Values.\n  JQ and VALUE accept every structured shape but not Lines. C accepts every shape.\n  CHECK preserves any input shape. H and HELP return Lines without evaluating preceding stages.\n\nUnsupported combinations fail before transformation with the stage, current shape, and accepted shapes. Empty is identity only for row-preserving stages. See docs/DSL.md for the complete result-shape matrix.",
        ),
        "jq" => Some(
            "JQ stage:\n  | JQ <expression> - run a jq-compatible transform with the in-process jaq interpreter.\n\nExamples:\n  object list --class Hosts --json | JQ 'map({Name, os_version})'\n  object list --class Hosts --json | JQ '.[] | .Name'\n\nJQ runs against the semantic payload after earlier stages.\nZero outputs become empty output. One output is shaped from its JSON type.\nMultiple outputs become semantic rows or values. Existing column metadata is cleared.",
        ),
        "redirects" => Some(
            "Redirects:\n  > <file> - write rendered output to a file.\n  >> <file> - append rendered output to a file.\n  > each:<template> - write one file per semantic row or value.\n\nOperators must be standalone, whitespace-delimited tokens.\nParent directories must exist. Compact legacy comparisons and spaced F WHERE comparisons are parsed as predicates. A later standalone > or >> redirects only after the preceding pipeline is complete.\nFile output follows the configured color mode: auto and never strip ANSI; always preserves it.\n\nExamples (REPL/script syntax):\n  object list --class Hosts | F WHERE age > 3 > adults.json\n  object list --class Hosts | P Name os_version > hosts.txt\n  object list --json --class Hosts | P Name os_version > each:/tmp/host-{Name}.json\n\nIn a POSIX one-shot command, escape or quote |, >, and >>.\nThis lets the shell pass those operators to Hubuum CLI.",
        ),
        _ => None,
    }
}
