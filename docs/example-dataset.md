# Explore the Atlas example inventory

Use the same small inventory as the server documentation and the other Hubuum
interfaces. Atlas demonstrates **classes, objects, relations, and access** with
four classes and ten connected objects.

## Load the shared dataset

Use the [server v0.0.18 Atlas guide](https://hubuum.github.io/hubuum/v0.0.18/getting-started/example-dataset/)
to load the inventory into an evaluation server and grant your account access.
This matches this release's server target. The server guide owns the downloads,
model, permissions, and expected data. Import adds the dataset atomically;
**restoring its backup replaces all application data**.

Use the case-sensitive names `Service`, `Server`, `Location`, and `Context`.
Resolve numeric IDs from responses; they vary between installations.

## Find classes, then their objects

After [connecting the CLI](getting-started.md), find the Service class and
list the Server objects:

```sh
hubuum-cli search --target class --where 'name == "Service"'
hubuum-cli object list --class Server
```

There are three servers: web-01, web-02, and worker-01. Filter by a JSON value:

```sh
hubuum-cli search --target object --class Server --where 'data.memory_gib >= 16' --sort name asc
```

The result contains web-01 followed by web-02. Both support the Atlas service;
worker-01 supports Beacon and has only 8 GiB of memory. Discover Atlas itself:

```sh
hubuum-cli search Atlas --kind object
```

The Server class has a shared monthly_cost computed field:

```sh
hubuum-cli object list --class Server --computed all --output json
```

After background computation, web-01's monthly_cost is 50: compute 45 plus
storage 5, in fictional cost units. Continue with [search](search.md),
[output pipelines](output-pipeline.md), and [schema evolution](schema-evolution.md)
using the same model.

## Relations and access

Use a non-admin account in `atlas-readers` to read the full inventory, or one
in `atlas-operators` to maintain Server/Location objects in the operations child.
Other memberships and token scopes affect the result. See the canonical
[relationships](https://hubuum.github.io/hubuum/v0.0.18/getting-started/example-dataset/#relations-connect-the-instances)
and [permission checks](https://hubuum.github.io/hubuum/v0.0.18/getting-started/example-dataset/#explore-collection-permissions)
for expected results and cleanup.
