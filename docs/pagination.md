# Pagination and output

For paginated commands, `--limit` requests a page size. The CLI currently
truncates values above 250 to the supported maximum with a
warning. Generated next-page commands retain that effective value. Paginated
commands also accept `--include-total` when an exact count is useful. Exact counts
can require additional server work, so they remain opt-in:

```sh
hubuum-cli object list --class Hosts --limit 25 --include-total
hubuum-cli task list --include-total --output json
```

Use `--all` to follow every remaining server cursor and buffer the complete
result before output pipelines run. `--limit` remains the page size when it is
combined with `--all`, and `--cursor <token> --all` starts from that cursor. The
CLI and client enforce automatic-pagination safety limits and reject repeated cursors.
Because complete results are held in memory, use `--all` deliberately for large
datasets:

```sh
hubuum-cli object list --class Hosts --all \| count
hubuum-cli audit list --cursor eyJpZCI6MTAwfQ --all --output json
```

If a pipeline is applied to a page that has more results without `--all`, the
CLI warns that the transformation only covered the current page.

Colored output defaults to terminal-aware `auto` mode and can be controlled per run or via `output.color`:

```sh
hubuum-cli --color never help
hubuum-cli --color always config paths
```

The current command vocabulary follows the Hubuum API:

- `collection` replaces the older namespace terminology.
- `export` replaces the older report terminology.
- `task list --kind export` filters export tasks.
- `task list --kind backup` filters backup tasks.
- `search --limit-per-kind` limits each result family independently.

Structured search runs predicates on the server and completes them with Tab in the
REPL. Quote the predicate, using double quotes for strings inside single quotes:

```sh
hubuum-cli search --target object --class Hosts --where 'data.cpu.cores >= 8 AND name ~ "^srv-"' --sort name asc
hubuum-cli search --query-file search.json --include-total --all --output json
hubuum-cli search server --stream --output jsonl
```

See [search and its terminal DSL](search.md) for fields, pagination, relation
queries, and streaming behavior, and [task discovery](tasks.md) for finding
background work by retained targets and options.

For transformations, redirects, JSON/CSV/TSV output, and table controls, use
[output pipelines](output-pipeline.md), the [DSL reference](DSL.md), and
[themes](themes.md). Pipeline operators must be escaped in a POSIX shell;
they are unescaped inside the REPL or a Hubuum script.
