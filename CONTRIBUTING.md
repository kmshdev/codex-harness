# Contributing

Thanks for improving `codex-harness`. Keep changes small, tested, and grounded in the CLI's public behavior.

## Local Setup

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt -- --check
```

Use `make install-local` only when you want to replace your local `~/.local/bin/codex-harness` binary.

## Change Guidelines

- Preserve `work-notes` as the default plan mode.
- Do not overwrite existing repository files unless a future explicit overwrite mode is designed and tested.
- Keep JSON envelopes stable for automation.
- Add or update tests for new commands, template behavior, and path-safety rules.
- Avoid committing generated smoke repositories, local work notes, or target build output.

## Release Checklist

1. Run `cargo test`.
2. Run `cargo clippy --all-targets --all-features -- -D warnings`.
3. Run `cargo fmt -- --check`.
4. Smoke test `codex-harness --json doctor` and one disposable `repo apply --dry-run`.
5. Update `CHANGELOG.md` before tagging a release.
