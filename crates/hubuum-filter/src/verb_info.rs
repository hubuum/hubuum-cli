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
            summary: "Choose fields to keep; !field drops fields.",
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
            "Search stages:\n  | pattern - keep rows where key paths or visible or hidden values match a regex.\n  | F <pattern> - same as bare search, useful when the pattern looks like syntax.\n  | F <field> <regex> - keep rows where one selector matches a regex.\n  | F <field><op><value> - compact =, !=, ~, >, >=, <, or <= predicate.\n  | V <pattern> - search scalar values only, ignoring key names.\n  | K <pattern> - search key paths only and project matching keys.\n  | reject <pattern> - remove rows matching a broad pattern.\n  | reject <field> <regex> - remove rows where one selector matches.\n  | ? [field] - keep truthy rows, or rows where a selector has a non-empty value.\n\nExamples:\n  object list --class Hosts | F os_version 26\n  object list --class Hosts | F data.cpu.cores>=8\n  object list --class Hosts | V 129.240\n  object list --class Hosts | K ipv4\n  object list --class Hosts | ? data.network.interfaces[]",
        ),
        "project" => Some(
            "Projection stages:\n  | P <field> [field...] - keep selected fields as table columns.\n  | P <field> !<field> - keep selected fields and drop excluded fields.\n  | VALUE <path> - extract selector matches as a value list.\n  | VAL <path> - short alias for VALUE.\n\nExamples:\n  object list --class Hosts | P Name os_version data.network.interfaces[*].ipv4\n  object list --class Hosts | P Name data !data.secrets\n  object list --class Hosts | VALUE data.network.interfaces[*].ipv4",
        ),
        "sort" => Some(
            "Sort stages:\n  | S <field> - sort rows ascending by one selector.\n  | S !<field> - sort rows descending by one selector.\n  | sort <field> asc|desc - explicit sort direction form.\n  | S <field> AS num|str|ip - sort with numeric, string, or IP address casting.\n\nExamples:\n  object list --class Hosts | S os_version\n  object list --class Hosts | S data.cpu.cores AS num\n  object list --class Hosts | S data.network.interfaces[0].ipv4 AS ip\n  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts | S Hosts desc AS num",
        ),
        "limit" => Some(
            "Limit and count stages:\n  | L [count] [offset] - keep a window of rows from the current result.\n  | head [count] [offset] - readable alias for L.\n  | tail [count] - keep rows from the end of the current result.\n  | C - replace rows with a count.\n  | count - readable alias for C.\n\nExamples:\n  object list --class Hosts | L 10\n  object list --class Hosts | L 10 20\n  object list --class Hosts | os_version contains 26 | C",
        ),
        "group" => Some(
            "Grouping stages:\n  | G <field> [AS alias] - group rows by one selector, optionally naming the output column.\n  | A count|sum(field)|avg(field)|min(field)|max(field) [AS alias] - add aggregates to each group.\n  | Z - collapse groups to one summary row per group.\n  | U <array-field> - unroll array members into one row per member.\n\nExamples:\n  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts\n  object list --class Hosts | G os_version AS \"OS Version\" | A count AS Hosts | S Hosts desc AS num\n  object list --class Hosts | G data.network.interfaces[*].ipv4 AS IPv4 | C\n  object list --class Hosts | U data.network.interfaces | P Name ipv4 mac",
        ),
        "selectors" => Some(
            "Selectors:\n  name                         field lookup\n  data.owner                   dotted path\n  data.network.interfaces[0]   array index\n  data.network.interfaces[-1]  negative index\n  data.network.interfaces[*]   fan out array\n  data.network.interfaces[]    fan out array\n  data.network.interfaces[:2]  slice",
        ),
        "jq" => Some(
            "JQ stage:\n  | JQ <expression> - run a jq-compatible transform with the in-process jaq interpreter.\n\nExamples:\n  object list --class Hosts --json | JQ 'map({Name, os_version})'\n  object list --class Hosts --json | JQ '.[] | .Name'\n\nJQ runs against the semantic payload after earlier stages.\nZero outputs become empty output. One output is shaped from its JSON type.\nMultiple outputs become semantic rows or values. Existing column metadata is cleared.",
        ),
        "redirects" => Some(
            "Redirects:\n  > <file> - write rendered output to a file.\n  >> <file> - append rendered output to a file.\n  > each:<template> - write one file per semantic row or value.\n\nOperators must be standalone, whitespace-delimited tokens.\nParent directories must exist. Compact comparisons such as F age>3 are not redirects.\nFile output follows the configured color mode: auto and never strip ANSI; always preserves it.\n\nExamples (REPL/script syntax):\n  object list --class Hosts | P Name os_version > hosts.txt\n  object list --json --class Hosts | P Name os_version > each:/tmp/host-{Name}.json\n\nIn a POSIX one-shot command, escape or quote |, >, and >>.\nThis lets the shell pass those operators to Hubuum CLI.",
        ),
        _ => None,
    }
}
