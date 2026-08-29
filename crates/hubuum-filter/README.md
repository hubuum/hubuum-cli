# hubuum-filter

`hubuum-filter` is a renderer-independent parser and evaluator for semantic JSON
pipelines. A caller supplies JSON plus an explicit output shape, applies a
validated pipeline, and renders the resulting JSON however it chooses.

The crate does not depend on Hubuum commands, authentication, configuration,
terminal output, or redirect handling. Despite the package name, its public API
is intended for any CLI or service that uses `serde_json::Value` as an
intermediary format.

## Example

```rust
use hubuum_filter::{OutputEnvelope, OutputShape, Pipeline};
use serde_json::json;

let pipeline = Pipeline::parse("F active | P name | S name")?;
let input = OutputEnvelope::rows(
    vec![
        json!({"name": "beta", "state": "active"}),
        json!({"name": "alpha", "state": "active"}),
        json!({"name": "retired", "state": "disabled"}),
    ],
    vec!["name".to_string(), "state".to_string()],
);

let output = pipeline.apply(input)?;

assert_eq!(output.shape(), OutputShape::Rows);
assert_eq!(output.columns(), ["name"]);
assert_eq!(
    output.value(),
    &json!([{"name": "alpha"}, {"name": "beta"}])
);

# Ok::<(), hubuum_filter::PipelineError>(())
```

`Pipeline::parse` accepts stage text with or without a leading pipe. Existing
applications that split a complete command line can use `split_pipeline` and
the lower-level `apply_pipeline` functions.

## Semantic Shapes

`OutputEnvelope` distinguishes these shapes:

- `Empty`: no semantic result;
- `Lines`: an inherently textual stream represented as JSON strings;
- `Rows`: an ordered array of records;
- `Detail`: one structured record;
- `Message`: one scalar or structured message;
- `Values`: an ordered JSON value array; and
- `Groups`: group summaries with attached member rows.

Every stage publishes its accepted input and possible result shapes through
`PipeStage::accepted_input_shapes` and `PipeStage::resulting_shapes`.
Unsupported transitions fail before evaluation rather than silently preserving
the input.

## Pipeline Language

The current native stages provide:

- broad, value-only, key-only, field, reject, and truthiness filtering;
- typed boolean predicates with explicit casts, null/missing tests, fanout,
  quoted JSON literals, and `NOT`/`AND`/`OR` composition;
- projection with validated output aliases and selector-based value extraction;
- stable multi-key sorting with strict scalar, date, version, natural, and
  IPv4/IPv6 casts, fanout reduction, and null order;
- head, tail, and count;
- grouping, aggregation, collapse, and array unroll; and
- JQ-compatible transforms through the in-process `jaq` evaluator.

Selectors support dotted fields, indexes, negative indexes, array fanout with
`[]` or `[*]`, and slices. Selectors are validated when parsed or constructed.

The full delivered syntax and shape matrix live in the repository's
[DSL guide](https://github.com/hubuum/hubuum-cli/blob/main/docs/DSL.md).

## Caller-specific Search Policy

Generic searches inspect every JSON key by default. A caller can explicitly
exclude presentation or bookkeeping keys from recursive broad searches and
key- or value-only searches without embedding application policy in the crate:

```rust
use hubuum_filter::PipelineSettings;

let settings = PipelineSettings::new()
    .with_ignored_search_keys(["created_at", "updated_at"])?;

# Ok::<(), hubuum_filter::PipelineError>(())
```

Pass the settings to `Pipeline::apply_with_settings` or
`apply_pipeline_with_settings`. Values selected through an envelope's explicit
visible columns remain searchable.

## Bounded JQ

`validate_bounded_jq_expression` and `evaluate_bounded_jq` expose a separately
bounded JQ subset for callers that evaluate untrusted workflow expressions.
`JqLimits` requires explicit positive expression, input, output-count, and
output-byte limits.

## Stability And Publication

Version `0.1` is the proposed first public API. The package is prepared for
`cargo package` verification but is not published by this repository change.
The native DSL is expected to evolve compatibly; typed request models and shape
contracts are the stable integration boundary.

## License

MIT
