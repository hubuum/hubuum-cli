# Install and update CLI binaries

## Release binaries

Successful pushes to `main` publish rolling binaries in the
[`main-latest` release](https://github.com/hubuum/hubuum-cli/releases/tag/main-latest).
Version tags such as `v0.0.14` publish immutable, versioned GitHub releases.

Each release provides four small, stripped archives and matching SHA-256 files:

- Linux x86_64 and ARM64 binaries are statically linked with musl.
- The Apple Silicon macOS binary depends only on Apple-provided system libraries.
- The Windows x86_64 binary uses the MSVC ABI with a statically linked C runtime;
  Windows system DLLs remain platform dependencies.

Rolling builds identify their source commit using SemVer build metadata, for example
`v0.0.14+main.g0123456789ab`. Tagged releases use the clean package version. Show the
current build identity without logging in, or also query the configured server:

```sh
hubuum-cli version
hubuum-cli version --server
hubuum-cli version --output json
```

The same `version` commands are available in the REPL. The server version comes from
the server's unauthenticated OpenAPI metadata.

### Updating in place

Starting with v0.0.13, check for or install the latest stable GitHub release:

```sh
hubuum-cli self-update --check
hubuum-cli self-update
hubuum-cli self-update --check --output json
```

These commands also work in the REPL and require no Hubuum login. `--check`
reads release metadata without downloading an archive or changing the executable.
Installation verifies the archive against its published SHA-256 file before
replacing the running executable on disk. Restart the CLI or REPL afterward;
the current process continues running its original version.

The installation directory must be writable. For a package-managed installation,
use its package manager. Supported targets match the four release platforms above;
Linux GNU builds receive the corresponding static musl binary. Other targets
must use their original installation method. The updater uses the
[`self_update` crate](https://docs.rs/self_update/1.3.0/self_update/).

Only strictly newer stable versions are installed. Prereleases and `main-latest`
are never destinations; a rolling build waits for a stable release with a higher
version, ignoring its build metadata. GitHub requests optionally use `GH_TOKEN`
or `GITHUB_TOKEN` (in that order) for API rate limits. Hubuum credentials are
not used. JSON output reports `status` (`up_to_date`, `update_available`, or
`updated`), both versions, the executable path, target, and `restart_required`.
