#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerbSummary {
    pub names: &'static str,
    pub topic: &'static str,
    pub summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpTopic {
    pub name: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
}

pub fn help_topics() -> &'static [HelpTopic] {
    &[
        HelpTopic {
            name: "search",
            title: "Search And Filter",
            summary: "Bare search, F, V, K, reject, and ?.",
        },
        HelpTopic {
            name: "project",
            title: "Projection And Values",
            summary: "P, VALUE, and VAL.",
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
            summary: "G, A, grouped C, and Z.",
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
            names: "bare, F, grep",
            topic: "search",
            summary: "Search rows by keys and values, or filter a specific field.",
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
            summary: "Aggregate grouped rows.",
        },
        VerbSummary {
            names: "U",
            topic: "group",
            summary: "Unroll array values into one row per member.",
        },
        VerbSummary {
            names: "JQ",
            topic: "jq",
            summary: "Apply a jq-compatible expression.",
        },
    ]
}

pub fn topic_help(topic: &str) -> Option<&'static str> {
    match topic {
        "search" => Some(
            "Search stages:\n  | pattern - keep rows where key paths or visible or hidden values match a regex.\n  | F <pattern> - same as bare search, useful when the pattern looks like syntax.\n  | F <field> <regex> - keep rows where one selector matches a regex.\n  | F <field><op><value> - compact legacy =, !=, ~, >, >=, <, or <= predicate.\n  | F WHERE <predicate> - typed boolean predicate with NOT, AND, OR, and parentheses.\n  | reject WHERE <predicate> - negate one complete typed predicate.\n  | V <pattern> - search scalar values only, ignoring key names.\n  | K <pattern> - search key paths only and project matching keys.\n  | reject <pattern> - remove rows matching a broad pattern.\n  | reject <field> <regex> - remove rows where one selector matches.\n  | ? [field] - keep truthy rows, or rows where a selector has a non-empty value.\n\nTyped tests support =, ==, !=, <, <=, >, >=, ~, !~, MATCHES, IN, IS NULL, and IS MISSING. Cast with AS str|num|bool|ip|datetime|version|natural. JSON numbers, booleans, and null are unquoted; strings are quoted.\n\nExamples:\n  object list --class Hosts | F os_version 26\n  object list --class Hosts | F data.cpu.cores>=8\n  object list --class Hosts | F WHERE data.cpu.cores AS num >= 8 AND state IN [\"ready\", \"running\"]\n  object list --class Hosts | reject WHERE owner IS MISSING OR disabled == true\n  object list --class Hosts | V 129.240\n  object list --class Hosts | K ipv4\n  object list --class Hosts | ? data.network.interfaces[]",
        ),
        "project" => Some(
            "Projection stages:\n  | P <field> [field...] - keep selected fields as table columns.\n  | P <field> AS <name>, <field> AS <name> - rename selected values as top-level output fields; commas are required when any alias is used.\n  | P <field> !<field> - keep selected fields and drop excluded fields; drop terms cannot use AS.\n  | VALUE <path> - extract selector matches as a value list.\n  | VAL <path> - short alias for VALUE.\n\nAliases retain selector cardinality: no match becomes null, one match stays scalar, and multiple matches become an array. Empty, duplicate, and conflicting grouped output names fail before rendering.\n\nExamples:\n  object list --class Hosts | P Name os_version data.network.interfaces[*].ipv4\n  object list --class Hosts | P Name AS Host, data.network.interfaces[].ipv4 AS Addresses\n  object list --class Hosts | P Name data !data.secrets\n  object list --class Hosts | VALUE data.network.interfaces[*].ipv4",
        ),
        "sort" => Some(
            "Sort stages:\n  | S <field> - sort rows ascending by one selector.\n  | S !<field> - sort rows descending by one selector.\n  | S <key>, <key> - stable lexicographic multi-key sort.\n\nEach key accepts asc|desc, AS str|num|bool|ip|datetime|version|natural, USING first|min|max, and NULLS FIRST|LAST in that order. Missing and null keys default to last for both directions. USING first is the fanout compatibility default. AS ip strictly validates std::net::IpAddr values, orders IPv4 before IPv6 ascending, and keeps mapped IPv6 in the IPv6 family.\n\nExamples:\n  object list --class Hosts | S os_version\n  object list --class Hosts | S state asc, updated_at desc AS datetime, Name AS natural\n  object list --class Hosts | S data.network.interfaces[].ipv4 AS ip USING min, Name\n  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts | S Hosts desc AS num",
        ),
        "distinct" => Some(
            "Stable distinct stages:\n  | D - keep the first occurrence of each complete visible JSON value.\n  | D <selector>, <selector> - keep the first occurrence of each ordered selector tuple.\n  | distinct <selector> AS <cast> - use a strict typed identity.\n\nKeys accept AS str|num|bool|ip|datetime|version|natural. Fanout selectors contribute their complete ordered sequence; missing differs from JSON null. Object field order does not affect whole-value equality. Empty, lines, rows, values, and groups are supported; grouped equality sees summaries and never merges members. Detail and message output are rejected.\n\nExamples:\n  object list --class Hosts | D\n  object list --class Hosts | D owner, os_version\n  object list --class Hosts | D data.network.interfaces[].ipv4 AS ip\n  object list --class Hosts | G rack | A count AS Hosts | D Hosts AS num",
        ),
        "limit" => Some(
            "Limit and count stages:\n  | L [count] [offset] - keep a window of rows from the current result.\n  | head [count] [offset] - readable alias for L.\n  | tail [count] - keep rows from the end of the current result.\n  | C - replace rows with a count.\n  | count - readable alias for C.\n\nExamples:\n  object list --class Hosts | L 10\n  object list --class Hosts | L 10 20\n  object list --class Hosts | os_version contains 26 | C",
        ),
        "group" => Some(
            "Grouping stages:\n  | G <field> [AS alias] - group rows by one selector, optionally naming the output column.\n  | A count|sum(field)|avg(field)|min(field)|max(field) [AS alias] - add aggregates to each group.\n  | Z - collapse groups to one summary row per group.\n  | U <array-field> - unroll row arrays before G or visible summary arrays after G.\n\nAfter G, filters, truthiness, key/value search, projection, sorting, and unroll operate on visible group and aggregate aliases. Filters retain or remove whole groups; hidden member rows never change. Grouped C emits each summary with its member count.\n\nExamples:\n  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts\n  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts | F Hosts>=2 | S Hosts desc AS num\n  object list --class Hosts | G data.network.interfaces[*].ipv4 AS IPv4 | C\n  object list --class Hosts | U data.network.interfaces | P Name ipv4 mac",
        ),
        "selectors" => Some(
            "Selectors:\n  name                         field lookup\n  data.owner                   dotted path\n  data.network.interfaces[0]   array index\n  data.network.interfaces[-1]  negative index\n  data.network.interfaces[*]   fan out array\n  data.network.interfaces[]    fan out array\n  data.network.interfaces[:2]  slice",
        ),
        "shapes" => Some(
            "Shape contracts:\n  Shapes are Empty, Lines, Rows, Detail, Message, Values, and Groups.\n  Legacy F, V, and reject accept every shape. Typed F WHERE and reject WHERE require structured shapes. K and ? accept every shape except Lines.\n  L, head, tail, and whole-line S accept Empty, Lines, Rows, Values, and Groups.\n  Field S and U accept Empty, Rows, Values, and Groups.\n  P accepts Empty, Rows, Detail, Message, and Groups.\n  G accepts Empty, Rows, Detail, Message, and Values; A and Z require Groups.\n  JQ and VALUE accept every structured shape but not Lines. C accepts every shape.\n\nUnsupported combinations fail before transformation with the stage, current shape, and accepted shapes. Empty is identity only for row-preserving stages. See docs/DSL.md for the complete result-shape matrix.",
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
