# Alias examples

Each file contains one complete Hubuum CLI command line and can be loaded as a
personal command alias with `alias set --command file://FILE`.

## `outdated-kernels`

Show hosts with kernels older than the newest observed for their OS release.

```sh
hubuum-cli alias set --name outdated-kernels \
  --description 'Show hosts with kernels older than the newest observed for their OS release' \
  --command file://examples/aliases/outdated-kernels.hubuum
```
