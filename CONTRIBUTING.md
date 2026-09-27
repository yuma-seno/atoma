# Contributing

## Setup

```bash
git clone https://github.com/yuma-seno/atoma
cd atoma
cargo build
```

Optional local checks:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Validation commands

Focused validation:

```bash
cargo test --test integration_test
```

Full validation:

```bash
cargo test
```

If you changed CLI behavior, run at least one manual smoke check:

```bash
cargo run -- --help
cargo run -- validate --agent-def <path-to-agent.md>
```

## Architecture map

- `src/application`: use-case orchestration (`run`, validation, runtime tool composition)
- `src/domain`: core contracts (agents, sessions, skills, tools, ports)
- `src/infra`: adapters (LLM providers, MCP transport, persistence, config/template)
- `src/main.rs`: CLI wiring, config resolution, process exit behavior
- `tests`: integration tests and test doubles

## Change checklist

- Keep changes aligned with existing layer boundaries (`domain` free of infra details).
- Add or update tests for behavioral changes.
- Verify docs/examples against actual command and flag names.
- Confirm error and exit behavior for changed runtime paths.
- Keep generated or unrelated formatting churn out of PR.

## Pull request expectations

- Explain user-visible behavior changes first.
- Include exact reproduction/verification commands you ran.
- Mention config/env prerequisites for reviewers.
- Keep PR scope tight; split unrelated refactors.
