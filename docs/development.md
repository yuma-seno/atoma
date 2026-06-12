# Development Guide

This guide covers building, testing, and working on Atoma locally.
All container examples use Docker; substitute `podman` for `docker` throughout.

---

## Prerequisites

- [Rust](https://rustup.rs) (version pinned in `rust-toolchain.toml`)
- Docker or Podman (optional — for containerised builds)

---

## Building locally

```bash
cargo build           # debug build → target/debug/atoma
cargo build --release # release build → target/release/atoma
```

---

## Running tests

```bash
cargo test
```

Test coverage includes:

| Layer | Test type | Location |
|---|---|---|
| `domain/session` | Unit — Message factory methods, `to_llm_value()` | `src/domain/session.rs` |
| `infra/hooks` | Unit — glob patterns, allowlist/denylist logic | `src/infra/hooks.rs` |
| `infra/persistence` | Unit — session roundtrip, agent def parsing | `src/infra/persistence/` |
| `infra/template` | Unit — system prompt rendering | `src/infra/template.rs` |
| `application/runner` | Unit — `extract_comment_id` | `src/application/runner.rs` |
| Integration | E2E — single response, tool call loop, max iterations, content filter | `tests/integration_test.rs` |

---

## Container builds

### Build images

```bash
# Production image (multi-stage, minimal — copies only the binary)
docker build -t localhost/atoma -f Dockerfile .

# Development image (Rust toolchain + source baked in)
docker build -t localhost/atoma-dev -f Dockerfile.dev .
```

### Run tests in container

```bash
# Uses the default CMD from Dockerfile.dev (cargo test)
docker run --rm localhost/atoma-dev
```

### Build with a mounted source tree

Mount your working directory so cargo compiles fresh changes without rebuilding the image.

**Docker:**
```bash
docker run --rm \
  -v "$PWD:/app" -w /app \
  localhost/atoma-dev \
  cargo build --release
```

**Podman (rootless):**

In rootless Podman the container runs as your host UID inside a user namespace.
Files written to a bind-mounted directory are owned by you on the host.
However, if `target/` was previously created by a different process (e.g. `sudo cargo build`
or a Docker daemon build), its files may be owned by root and block compilation.

Two clean approaches:

```bash
# Option A: named volume for target/ — isolates build artifacts from the host tree
podman run --rm \
  -v "$PWD:/app:z" \
  -v atoma-target:/app/target \
  -w /app \
  localhost/atoma-dev \
  cargo build --release

# Option B: --userns=keep-id — maps container UID to your host UID
podman run --rm --userns=keep-id \
  -v "$PWD:/app:z" \
  -w /app \
  localhost/atoma-dev \
  cargo build --release
```

> `:z` re-labels the volume for SELinux. Omit it on non-SELinux systems.

If you hit permission errors, remove any root-owned `target/` first:
```bash
sudo rm -rf target/
```

### Interactive shell

```bash
docker run --rm -it \
  -v "$PWD:/app" -w /app \
  localhost/atoma-dev bash
```

---

## Project structure

```
src/
  domain/          # Pure entities and port traits — no I/O, no external deps
    agent.rs       # AgentDef, ParsedAgentDef
    ports.rs       # LlmPort, McpPort, SessionPort, AgentDefPort, ToolDefPort, McpFactory
    session.rs     # Message, ToolCall, Session
    tool.rs        # Hooks, ToolDef
  infra/           # Port implementations — I/O, HTTP, file system
    llm/           # LLM clients (OpenAI-compat, Anthropic, GitHub Copilot)
    mcp.rs         # McpRegistry (stdio MCP client) + McpRegistryFactory
    hooks.rs       # Hook script execution, allowlist/denylist filtering
    template.rs    # System prompt rendering
    persistence/   # File-system adapters for session, agent def, tool def
  application/     # Use case orchestration — depends only on domain ports
    runner.rs      # Main inference loop
    validator.rs   # Agent definition validation (atoma validate)
  cli.rs           # CLI argument parsing (clap)
  lib.rs           # Library root (re-exports for integration tests)
  main.rs          # Binary entry point — wires infra adapters, calls application layer
tests/
  common/          # Shared test helpers (MockLlmClient, MockMcpRegistry)
  integration_test.rs
```

The layering follows Clean Architecture:
`domain` ← `application` ← `infra` ← `main.rs`

No layer may import from a layer above it.
`application` depends only on `domain` port traits; concrete implementations live in `infra`.

---

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md).
