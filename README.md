# codex-harness

`codex-harness` sets up Codex-ready harness documentation in a repository using
the OpenAI advanced pack from `walkinglabs/learn-harness-engineering`, adapted
for a Codex `work-notes` workflow.

The default plan mode is `work-notes`: scratch specs, implementation plans,
research notes, and execution logs belong under
`$CODEX_HOME/work-notes/<repo-slug>/<branch-or-thread>/`. The CLI only writes
durable system-of-record files into the target repo unless `--plan-mode tracked`
is requested.

## Install

From a released binary, download the archive for your platform from GitHub
Releases, unpack it, and place `codex-harness` somewhere on your `PATH`.

From source:

```bash
cargo install --path .
```

For local development:

```bash
make install-local
```

This installs `codex-harness` into `~/.local/bin`.

## What It Writes

By default, `repo apply` creates missing durable harness files such as
`AGENTS.md`, `ARCHITECTURE.md`, `docs/PLANS.md`, `docs/SECURITY.md`, and
reference placeholders. It does not overwrite existing files with different
content; those are reported as conflicts.

Use `--dry-run` before writing to a repository you care about.

## Common Commands

```bash
codex-harness --json doctor
codex-harness --json repo inspect --target .
codex-harness --json repo plan --target . --pack openai-advanced --plan-mode work-notes
codex-harness --json repo apply --target . --pack openai-advanced --plan-mode work-notes --dry-run
codex-harness --json repo apply --target . --pack openai-advanced --plan-mode work-notes
codex-harness --json audit --target .
codex-harness --json validate --target .
codex-harness --json sop list
codex-harness --json sop show encode-knowledge-into-repo
```

`audit` and `validate` are readiness gates. They also run an internal
template-drift check because upstream structural audits can report high scores
for docs that still contain starter text. If a target repo still contains exact
bundled template docs, the command exits nonzero, the JSON envelope has
`ok: false`, and `data.template_check.stale_templates` lists the files that must
be customized, promoted, or removed.

## JSON Policy

Successful commands emit:

```json
{
  "ok": true,
  "command": "doctor",
  "data": {},
  "warnings": []
}
```

Errors emit:

```json
{
  "ok": false,
  "command": "repo",
  "error": {
    "code": "command_failed",
    "message": "what failed",
    "hint": "Run with --help for command usage."
  }
}
```

The CLI does not print tokens, cookies, or secret values.

## Development

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

See `CONTRIBUTING.md` for release and contribution expectations.

## License

MIT. See `LICENSE`.
