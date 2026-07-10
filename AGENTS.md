# Repository Guidelines

## Verification

- All tests must pass before changes are considered complete. Run the workspace-wide test suite.
- `cargo clippy` must pass for all code before changes are considered complete.
- Run clippy as `cargo clippy --all-targets -- -D warnings`.
- `rustfmt` must pass for all Rust code. Keep formatting mechanical and avoid hand-formatting that fights `rustfmt`.

## Architecture

- Use workspace crates whenever possible.
- Workspace crates should expose small, explicit interfaces with private fields. Prefer typed request or builder APIs over long positional argument lists when callers must provide several settings.
- Keep workspace crate boundaries clean of app-specific errors and global config unless the crate explicitly owns that layer.
- Avoid leaking third-party implementation types from workspace crate APIs unless they are the intentional integration surface.

## Rust Standards

- Follow Rust best practices and the conventions already present in this repository.
- Prefer designs built around newtypes instead of passing primitive values through the domain unchecked.
- Newtypes should usually have validating constructors, private fields, and explicit accessors or setters where mutation is part of the model.
- Endpoints should accept newtypes whenever possible so validation happens at the boundary, as early as possible, with clear and actionable error messages.
- Put behavior on types with `impl` blocks when it naturally belongs to the type. Prefer this over collections of bare functions that operate on loosely related data.
- Keep invariants close to the data they protect. Constructors and setters should reject invalid states rather than relying on callers to remember preconditions.
- Use small, explicit APIs. Expose only what callers need, and keep representation details private unless there is a strong reason not to.
- Prefer `use` imports over inline fully-qualified paths for functions, types, and macros. Only fully qualify a path inline when needed to resolve a genuine name ambiguity (or for a one-off reference where a `use` would mislead).
