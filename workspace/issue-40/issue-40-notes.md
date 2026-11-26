# Issue #40 notes (engineer run)

Change: .github/atomaton/config.yaml `environment.setup_commands` now carries
`rustup toolchain install 1.95.0 --profile minimal --component rustfmt --component clippy`.

Facts measured in this run (image: image has stable 1.100.0, rustup `profile = "minimal"` in /home/runner/.rustup/settings.toml):
- run_environment_setup.ts reads the config from ${ATOMATON_MACHINERY_ROOT}/.github/atomaton/config.yaml,
  i.e. the DEFAULT-BRANCH checkout at /home/runner/work/_temp/atomaton-machinery — not the work tree.
  => the entry takes effect only after a person merges it to the default branch.
- The setup step (atomaton-runner.yml:475) runs as `runner` before the tool users step (559) and before
  "Run agent" (807); a non-zero exit fails the job (run_environment_setup.ts:137-140).
- Agent process env comes from launch_atoma.ts buildAgentEnv: HOME is passed, RUSTUP_HOME is NOT,
  CARGO_HOME = $RUNNER_TEMP/atomaton-tool-cache/cargo. Agent runs as uid atomaton-tools, which has only
  --x on /home/runner (getfacl), so /home/runner/.rustup (runner-owned, 755) is readable but not writable.
- Plain `rustup toolchain install 1.95.0` under profile=minimal installs 3 components: no rustfmt, no clippy.
  With the two --component flags: 5 components, both present. Re-run is idempotent (0.2s, exit 0).
- With the resulting 1.95.0 install and RUSTUP_HOME chmod a-w (read-only, as atomaton-tools sees it),
  `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --locked` all exit 0 under cargo 1.95.0.
- Could NOT run the setup step as `runner`: sudoers grants atomaton-tools only `(atomaton-untrusted)`.
