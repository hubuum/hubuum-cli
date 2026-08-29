# RFC 0001: Native Pipeline DSL Increment

- Status: Proposed
- Tracking issue: [#46](https://github.com/hubuum/hubuum-cli/issues/46)
- Scope: predicates, sorting, projection aliases, distinct, and aggregation

## Summary

This RFC defines the next compatible increment of the native JSON pipeline
language. It adds an explicit typed predicate grammar, stable multi-key sorting,
strict casts including IPv4 and IPv6, projection aliases, stable distinct, and
global aggregation. The existing shorthand remains valid.

The native language is for operations where the CLI can preserve semantic
shape, column metadata, completion, renderer independence, and redirect safety.
JQ remains the escape hatch for arbitrary JSON construction. Joins, unions,
window functions, and unbounded collection are outside this increment.

## Goals

- Make boolean filtering and typed comparisons explicit and predictable.
- Keep redirection unambiguous even when comparisons contain a spaced `>`.
- Make multi-key ordering stable and reject invalid casts with useful context.
- Preserve JSON values and column metadata through common transformations.
- Add only aggregation operations supported by current CLI workflows.
- Represent parsed syntax with validated types rather than string rewriting.
- Deliver the work in reviewable slices that each include help, completion,
  documentation, and tests.

## Compatibility Boundary

Existing syntax keeps its current meaning:

```text
| contact
| F contact
| F age>=3
| F age equals 3
| reject retired
| S name
| S name desc AS str
| P Name os_version
| G os_version | A count AS Hosts | Z
```

In particular, existing `F` comparisons retain scalar-text compatibility. The
new typed predicate grammar begins only after the `WHERE` marker:

```text
| F WHERE age AS num >= 3 AND (state == "active" OR state IS NULL)
```

This boundary prevents a typed literal such as `3` from changing whether the
legacy expression `F age=3` matches a JSON string, a JSON number, or both.
`reject WHERE predicate` is the negation of the complete typed predicate. Bare
search, legacy `F`, and legacy `reject` continue to use their current parser.

A current one-key sort is a one-element multi-key sort. Unaliased projection,
grouped aggregation, output names, selectors, and stage aliases remain valid.
`D` is available for distinct because unknown single-letter stages already
fail; accepting it does not reinterpret a previously valid pipeline.

## Lexical Rules

The typed grammar needs a dedicated lexer over the original stage source.
Using shell-style word splitting would discard the distinction between `3` and
`"3"` before typed literal parsing.

- Whitespace separates tokens except inside quotes.
- Keywords in the new grammar are ASCII case-insensitive. Stage aliases retain
  their documented spelling.
- Double-quoted strings accept JSON escapes, including Unicode escapes.
- Single-quoted strings accept `\\`, `\'`, and otherwise preserve their text.
- A selector in operand position may be bare or quoted. Quoting is necessary
  for a visible alias containing whitespace.
- A comma separates sort keys, aliased projection terms, aggregate expressions,
  and membership values. A trailing comma is an error.
- Parentheses group predicates. Brackets delimit an `IN` list and retain their
  existing selector meaning when attached to a selector component.
- Empty expressions, unterminated quotes, unexpected delimiters, and extra
  tokens fail during pipeline parsing with a byte position.

POSIX shell quoting is a separate outer layer. The examples in this document
are REPL or script syntax; a one-shot shell command must still quote or escape
its pipe and redirect characters.

## Predicate Grammar

The proposed grammar is:

```ebnf
typed-filter    = ("F" | "grep"), "WHERE", predicate ;
typed-reject    = "reject", "WHERE", predicate ;

predicate       = or-expression ;
or-expression   = and-expression, { "OR", and-expression } ;
and-expression  = not-expression, { "AND", not-expression } ;
not-expression  = { "NOT" }, primary ;
primary         = "(", predicate, ")" | test ;

test            = operand, [ "AS", cast ], test-tail ;
test-tail       = comparator, literal
                | ("~" | "!~" | "MATCHES"), string
                | [ "NOT" ], "IN", "[", literal-list, "]"
                | "IS", [ "NOT" ], ("NULL" | "MISSING") ;

operand         = selector ;
comparator      = "=" | "==" | "!=" | "<" | "<=" | ">" | ">=" ;
literal-list    = [ literal, { ",", literal } ] ;
literal         = string | number | "true" | "false" | "null" ;
cast            = "str" | "num" | "bool" | "ip" | "datetime"
                | "version" | "natural" ;
```

Precedence from highest to lowest is parentheses, `NOT`, a field test, `AND`,
then `OR`. Operators at the same level associate left to right. Comparisons do
not chain: `1 < age < 10` is an error and must be written as two tests joined by
`AND`.

Examples:

```text
| F WHERE state IN ["ready", "running"] AND retries AS num < 3
| F WHERE NOT (owner IS MISSING OR owner IS NULL)
| F WHERE "OS Version" AS natural >= "9.4"
| F WHERE address AS ip >= "2001:db8::1"
| reject WHERE status == "retired" OR disabled == true
```

### Typed Values

`null`, booleans, and numbers use their JSON types. Quoted values are strings.
Without `AS`, equality requires the same JSON type; `3` and `"3"` differ.
Ordered comparison without a cast requires two values of the same orderable
type. Arrays and objects are not implicitly stringified.

Casts apply to selected non-null values and to the comparison literal:

- `str` accepts JSON strings, numbers, and booleans and uses their canonical
  scalar text.
- `num` accepts a JSON number or a string containing one complete finite JSON
  number. Prefixes, suffixes, NaN, and infinity are invalid.
- `bool` accepts JSON booleans or the strings `true` and `false`, ignoring ASCII
  case.
- `ip` accepts only values parsed by `std::net::IpAddr`.
- `datetime` accepts RFC 3339 with an explicit offset and compares instants
  after normalization to UTC.
- `version` accepts Semantic Versioning 2.0 values.
- `natural` accepts strings and compares alternating digit and non-digit runs;
  digit runs compare by numeric magnitude and text runs compare by Unicode
  scalar value. Equal numeric runs use fewer leading zeroes first as a stable
  tie-break.

An invalid literal is a parse error. A selected non-null value that cannot be
cast aborts evaluation and reports the stage, selector, cast, row index, and
offending JSON value. Missing and null values are not cast failures. Boolean
evaluation short-circuits left to right, so an unevaluated branch cannot cause
a cast error.

### Selector Cardinality And Missing Values

Positive comparisons, regex matches, and `IN` are existential for fanout
selectors: any selected value may satisfy the test. `!=`, `!~`, and `NOT IN`
require at least one selected value and require every selected value to satisfy
the negative test. This preserves the current safe behavior where a missing
field does not satisfy `field != value`.

`IS NULL` tests for an actual selected JSON null. `IS MISSING` tests whether the
selector produced no values. Their `IS NOT` forms are exact logical opposites.
The general `NOT` operator negates the complete nested predicate, including its
missing-value result.

## Redirect Disambiguation

Redirects remain standalone `>` or `>>` tokens followed by exactly one sink.
Compact comparison tokens such as `age>3` and `age>=3` can never be redirects.

For spaced typed comparisons, redirect discovery scans candidates from right to
left. A token is a redirect only when:

1. the suffix is one valid file or `each:` sink;
2. the prefix is a complete command and complete pipeline under the typed
   predicate grammar; and
3. consuming the token as a predicate operator is not required to complete the
   active `WHERE` expression.

Thus the first `>` below is a comparison and the second is a redirect:

```text
object list --class Hosts | F WHERE age > 3 > adults.json
```

An incomplete comparison such as `F WHERE age >` is a predicate error, not a
redirect to an empty or missing sink. Redirect parsing must consume the same
typed pipeline AST used by execution; it must not duplicate a partial grammar.

## Ordered Multi-key Sorting

Sorting extends the current stage without changing its one-key form:

```ebnf
sort-stage      = ("S" | "sort"), sort-key, { ",", sort-key } ;
sort-key        = [ "!" ], selector,
                  [ "asc" | "desc" ],
                  [ "AS", cast ],
                  [ "USING", ("first" | "min" | "max") ],
                  [ "NULLS", ("FIRST" | "LAST") ] ;
```

Examples:

```text
| S state asc, updated_at desc AS datetime, Name asc AS natural
| S data.network.interfaces[].ipv4 asc AS ip USING min, Name
```

Keys compare lexicographically in declaration order. The sort is stable, so
rows equal on every key retain their input order. `!field` remains the short
form of `field desc`.

A selector producing no values supplies a null key. Nulls default to last for
both ascending and descending order; `NULLS FIRST` or `NULLS LAST` overrides
that per key. A fanout selector defaults to `USING first` for backward
compatibility. `USING min` and `USING max` cast every selected value and reduce
them under that key's ordering. An invalid selected cast aborts the stage using
the diagnostic policy defined above.

`AS ip` uses validated IPv4 and IPv6 addresses. IPv4 sorts by its numeric
32-bit value and IPv6 by its numeric 128-bit value. IPv4 sorts before IPv6 in
ascending order; descending reverses the family and address order. IPv4-mapped
IPv6 remains IPv6. Invalid addresses never become zero and never silently sort
as null.

## Projection Aliases

Projection gains an optional output name:

```ebnf
projection-stage = ("P" | "columns"), projection-term,
                   { [ "," ], projection-term } ;
projection-term  = [ "!" ], selector, [ "AS", output-name ] ;
```

When any term uses `AS`, commas are required between terms. This avoids guessing
where an alias ends and the next selector begins.

```text
| P Name AS Host, data.os_version AS "OS Version"
```

An alias creates one top-level output field with the selected JSON value. One
match remains scalar, multiple matches become an array, and no match becomes
null, matching current projection behavior. Unaliased terms retain their
current output name. Drop terms cannot use `AS`.

Aliases use the validated output-name type. Empty names and duplicate final
names fail during parsing. An alias may not overwrite another projected field,
group key, or aggregate field. Projection of grouped output continues to act on
visible summaries and keeps member rows attached for later aggregation.

Native computed expressions are deferred. `P selector AS name` only renames a
selected value; JQ remains the explicit mechanism for constructing new values.

## Stable Distinct

`D` and `distinct` remove duplicates while retaining the first occurrence:

```ebnf
distinct-stage = ("D" | "distinct"),
                 [ distinct-key, { ",", distinct-key } ] ;
distinct-key   = selector, [ "AS", cast ] ;
```

```text
| D
| D os_version
| D owner, address AS ip
```

Without keys, equality uses the complete visible JSON value. Object field order
does not affect equality. With keys, equality uses the ordered tuple of selected
results. A fanout result participates as its complete ordered sequence; it is
not reduced to the first value. Missing is a distinct sentinel and is not equal
to JSON null. Cast failures use the common strict diagnostic policy.

Distinct accepts `Empty`, `Lines`, `Rows`, `Values`, and `Groups`, retains the
input shape, order, and columns, and rejects `Detail` and `Message`. For Groups,
keys and whole-value equality see only the visible summary and duplicates are
removed as whole groups; hidden members are never merged.

## Grouped And Global Aggregation

Current grouped aggregation remains unchanged:

```text
| G os_version | A count AS Hosts | Z
```

Global aggregation is explicit and does not synthesize a fake group key:

```ebnf
global-aggregate = "A", "GLOBAL", aggregate-list ;
aggregate-list   = aggregate-term, { ",", aggregate-term } ;
aggregate-term   = aggregate-expression, [ "AS", output-name ] ;
```

`A GLOBAL` accepts `Empty`, `Rows`, and `Values` and returns exactly one `Rows`
record containing the aggregate aliases. It consumes its input collection.
Applying it to `Groups`, `Detail`, `Message`, or `Lines` is an actionable shape
error. Use `Z` before a global aggregate of visible group summaries.

The following additions have concrete repository workflows:

- `count(selector)` counts non-null selector matches. Fanout selectors count
  each selected value. This answers inventory completeness questions such as
  how many returned hosts report an owner or an interface address.
- `count_distinct(selector)` counts distinct non-null selected values using the
  same equality rules as `D`. This answers inventory cardinality questions for
  OS versions, owners, collections, and addresses.
- `first(selector)` and `last(selector)` return the first or last non-null match
  in current row and selector order. Sorting before `G` makes task or audit
  event boundaries explicit and reproducible.

Examples:

```text
object list --class Hosts \
  | A GLOBAL count AS Hosts, count(data.owner) AS Owned, \
      count_distinct(os_version) AS Versions

task events <task-id> \
  | S created_at asc AS datetime \
  | G kind \
  | A first(created_at) AS First \
  | A last(created_at) AS Last \
  | Z
```

`count` on an empty input is zero. `count(selector)` and
`count_distinct(selector)` are also zero. Numeric, `first`, and `last`
aggregates return null when they have no contributing values. Existing default
aggregate aliases remain; all final names must be unique.

`collect`, string joining, percentile, median, and arbitrary reducers are
deferred. Collection needs an explicit item/byte bound, while percentile needs
an agreed interpolation method and a demonstrated command-level workflow.
Adding them speculatively would enlarge the language without a stable contract.

## Typed Model And Library Boundary

The parser should produce small validated types with private fields:

- `Predicate`, `PredicateExpr`, `Comparison`, `TypedLiteral`, and `ValueCast`;
- `SortSpec`, `SortKey`, `SortDirection`, `FanoutReduction`, and `NullOrder`;
- `ProjectionSpec` and `ProjectionTerm` with an optional `OutputName`;
- `DistinctSpec` and `DistinctKey`;
- `AggregateRequest`, `AggregateSpec`, and validated aggregate functions.

Evaluation accepts those types and `serde_json::Value`; it must not depend on
Hubuum commands, global configuration, terminal rendering, or app errors. Cast
and selector behavior belongs in the reusable pipeline crate. The CLI owns
command tokenization, final rendering, help presentation, completion sources,
and redirect sinks.

Every new stage or variant must extend the stage/shape contract metadata. The
same metadata should constrain REPL stage completion after a statically known
transition where possible.

## Delivery Plan

Each item below is independently deliverable and should be tracked by its own
follow-up issue and PR against the then-current `main`:

1. [#55](https://github.com/hubuum/hubuum-cli/issues/55): typed `F WHERE` and
   `reject WHERE` predicate lexer, AST, evaluation, and parser-aware redirect
   disambiguation.
2. [#56](https://github.com/hubuum/hubuum-cli/issues/56): stable ordered
   multi-key sorting, strict common cast policy, fanout reduction, and null
   placement.
3. [#57](https://github.com/hubuum/hubuum-cli/issues/57): strict `IpAddr`
   sorting for IPv4 and IPv6, separated so address-ordering tests and diagnostics
   stay focused.
4. [#58](https://github.com/hubuum/hubuum-cli/issues/58): projection aliases
   using validated output names and collision checks.
5. [#59](https://github.com/hubuum/hubuum-cli/issues/59): stable `D`/`distinct`
   for lines and semantic collections.
6. [#60](https://github.com/hubuum/hubuum-cli/issues/60): `A GLOBAL` plus
   `count(selector)` and `count_distinct(selector)`.
7. [#61](https://github.com/hubuum/hubuum-cli/issues/61): ordered
   `first(selector)` and `last(selector)` aggregates, with task and audit workflow
   examples.

Dependencies are limited: item 3 builds on item 2's cast interface, and items 6
and 7 share the aggregate model. The other items can land independently after
rebasing onto updated `main`.

Every item includes:

- parser, model, evaluator, and stage/shape contract tests;
- legacy syntax and redirect regression tests;
- text, JSON, JSONL, CSV, and TSV renderer coverage where output changes;
- `each:` redirect coverage when names or cardinality change;
- `help pipe`, REPL completion, and `docs/DSL.md` updates; and
- workspace tests, clippy with warnings denied, rustfmt, and Markdown lint.

## Deferred Features

- Native computed projection expressions remain JQ territory.
- Joins, unions, and window functions remain JQ territory until a concrete
  Hubuum workflow demonstrates a metadata-preserving native design.
- Locale-sensitive collation is not part of `str` or `natural` ordering.
- Implicit coercion is not added to the typed grammar.
- Invalid casts do not gain a silent skip, zero, or null mode in this increment.

These choices keep the native layer small, deterministic, and suitable for a
standalone JSON pipeline crate.
