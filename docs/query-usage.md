# Query usage

Query-usage declarations describe JSON query patterns you expect to use on a
class. They are separate from schema validation and do not promise an index or
faster queries. The server introduced them in v0.0.19; this CLI targets v0.0.20.

## Manage declarations

Use a class name, a comma-separated JSON path, a scalar type, and operations:

```text
class query-usage create Hosts --path hardware,serial_number --type string --operations equals
class query-usage create Hosts --path cpu,cores --type numeric --operations equals,gte
class query-usage list Hosts --output json
```

Types are `string`, `numeric`, and `boolean`. String and boolean patterns accept
only `equals`; numeric patterns also accept `gt`, `gte`, `lt`, `lte`, and
`between`. Paths support at most 32 segments and 512 encoded bytes. A class can
hold at most 32 declarations. Listing requires `ReadClass`; mutations require
`UpdateClass`.

Replacement supplies the whole pattern. Use the declaration ID and revision
returned by listing, after reviewing its current contents:

```text
class query-usage replace Hosts --id 7 --expected-revision 1 --path cpu,cores --type numeric --operations equals,gte,between
class query-usage delete Hosts --id 7 --expected-revision 2
```

A stale revision fails with a conflict. Refresh the list and review the new
contents before retrying; the CLI never retries a conflicting write
with a newer revision automatically. Duplicate path/type declarations also
conflict. Schema compatibility is advisory and does not change the class schema
or its validation policy.

## Analyze workload evidence

Analysis requires an unscoped administrator with class read access:

```text
class query-usage analyze Hosts
class query-usage analyze Hosts --output json
class query-usage analyze Hosts --proposed file://patterns.json
```

The optional file contains a JSON array of at most 32 hypothetical patterns:

```json
[
  {
    "path": "hardware,serial_number",
    "value_type": "string",
    "operations": ["equals"]
  }
]
```

Analysis does not adopt proposals or suggestions. Create or replace declarations
explicitly after reviewing them. The report distinguishes `unavailable`,
`insufficient_evidence`, and `complete`; a complete report can have no
suggestions.

Observations are disabled by default. When enabled on the server, they are
sampled, bounded, value-free, process-local evidence and disappear on restart.
Durations measure whole queries, including other predicates and authorization;
they do not measure one predicate's cost. Read the effective observation
settings and report limitations before interpreting the result.

Text output summarizes assessments, suggestions, native resource references,
ownership, size, scan counts, cleanup eligibility, and adapter progress. JSON
preserves all returned details. An absent scan count means unknown, and a zero
count or missing observation does not prove an index is unused. Adapter states
are operational facts rather than portable readiness guarantees.

## Prepare native resources and restore intent

The CLI manages declarations and reads analysis. Native preparation runs
separately on the server through `hubuum-admin --reconcile-query-usage` with the
appropriate database role configuration. The PostgreSQL adapter supports
shared hash indexes for supported text equality patterns; a valid numeric or
boolean declaration does not imply native preparation is available.

[Format 9 backups](backup-restore.md) preserve declaration intent and attribution.
They do not preserve physical indexes or native ownership records. After restore,
review the declarations and run the server reconciliation workflow as appropriate.
