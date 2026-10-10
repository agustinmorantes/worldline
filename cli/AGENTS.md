# AGENTS.md

This file provides guidance to coding agents working in `cli/`.

## Crate Role

`cli/` is the workspace command-line interface. It is intentionally thin:

- argument parsing and subcommand definitions live here
- stdout/stderr formatting lives here
- business logic, persistence behavior, watcher behavior, and scanning logic belong in `../worldline-core`

If a change would also benefit the GUI, implement it in `worldline-core` and call it from the CLI.

## Layout

- `src/main.rs` - CLI entrypoint, subcommands, app-data setup, and output formatting
- `tests/cli_tests.rs` - end-to-end CLI tests

Current top-level commands are:

- `games`
- `versions`
- `slots`
- `prune`
- `history`
- `sync`
- `devices`
- `repo`
- `watch`

## Commands

Run from `cli/` unless noted otherwise.

```bash
cargo run -- --help
cargo run -- games list
cargo test
cargo clippy --all-targets --all-features
cargo fmt
```

From the workspace root, the equivalent targeted runs are:

```bash
cargo run -p cli -- --help
cargo test -p cli
```

## Implementation Guidance

- Keep command handlers straightforward and delegate to `WorldlineCore`.
- Reuse the existing initialization pattern that creates the data directory, opens SQLite, runs embedded migrations, and constructs repository implementations.
- The default data dir should stay aligned with the GUI app's local-data layout unless there is an explicit migration plan.
- Hidden testing flags such as `--debounce-secs` are part of the current testability story; do not remove them casually.

## Testing Guidance

- Prefer end-to-end tests that invoke the compiled binary and assert on process output plus filesystem effects.
- Use temporary directories instead of the real user data directory in tests.
- When changing command output, update tests to assert the new shape explicitly rather than weakening expectations.

## Output And UX Notes

- Keep console output stable and readable; this crate currently prints concise status lines rather than structured JSON.
- Send operational errors to stderr where appropriate and keep success output succinct.
- Avoid introducing interactive prompts unless explicitly required by the task.
