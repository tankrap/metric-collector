# Distribution and Wrapper Commands

vc-tokmeter is designed to support installed binaries and one-shot wrapper
commands. This repository is still in early implementation, so the source
checkout path is the currently usable route. The other commands below are the
planned v1 user-facing commands and should not be published as working install
instructions until packaging is wired.

## Current Source Checkout

Use this path while developing or testing from the repository:

```sh
git clone git@github.com:tankrap/metric-collector.git
cd metric-collector
cargo run -- --help
cargo test
```

Prerequisites:

- Rust stable toolchain with Cargo.
- A local repository where the tester can run baseline and treatment tasks.
- Codex hooks, Claude Code hooks, or local proxy wiring once the matching
  adapter is enabled.

## Codex Project Hook

From the repository you want to measure, run the source-checkout binary's
`init` command. During development that usually means invoking this checkout
with Cargo from the target repository:

```sh
cargo run --manifest-path /path/to/metric-collector/Cargo.toml -- init
```

`init` creates a project-local `.codex/hooks.json` containing Codex
`PreToolUse` and `PostToolUse` command hooks. The hook command points back to
the running `vc-tokmeter` binary and appends privacy-safe event records to
`.tokmeter/events.jsonl`.

Start Codex in that trusted project, run `/hooks`, and trust the tokmeter hook
definitions if Codex marks them for review. Then use Codex normally and produce
a local report:

```sh
cargo run --manifest-path /path/to/metric-collector/Cargo.toml -- report \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

## Local Proxy Runtime

The proxy runtime is intentionally localhost-only. `ProxyConfig::new` accepts
`localhost`, `127.0.0.1`, or `::1` bind hosts and rejects wildcard or LAN
addresses before binding.

The runtime supports HTTP and HTTPS upstream forwarding. It rewrites the
request target to the configured upstream base path, forwards provider
credentials to the upstream, relays the upstream response bytes to the client,
and captures only sanitized metadata locally:

- request method and query-redacted path;
- sensitive headers with values replaced by `[REDACTED]`;
- provider usage/cache token fields from the upstream response body;
- attribution-compatible proxy events and core event-log JSONL records.

Prompt bodies, response content, tool output text, query credential values, and
provider credentials are not persisted in proxy capture records. By default,
`tokmeter proxy` appends captured core event records to `.tokmeter/events.jsonl`.

To test Codex through the proxy against OpenAI:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- proxy \
  --bind-host 127.0.0.1 \
  --port 17683 \
  --upstream https://api.openai.com \
  --event-log .tokmeter/events.jsonl
```

Then start Codex in a separate terminal with a user-level base URL override:

```sh
codex -c openai_base_url='"http://127.0.0.1:17683/v1"'
```

Run a short Codex task, stop the proxy, and regenerate the report:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- report \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

## Static Binary

Planned v1 install path:

```sh
curl -L https://github.com/tankrap/metric-collector/releases/latest/download/vc-tokmeter-macos-arm64.tar.gz -o vc-tokmeter.tar.gz
tar -xzf vc-tokmeter.tar.gz
chmod +x vc-tokmeter
./vc-tokmeter --help
```

Installed binary usage:

```sh
./vc-tokmeter init
./vc-tokmeter doctor
./vc-tokmeter run --profile baseline --task task-id
./vc-tokmeter report --compare
```

This path is for repeat local use. The binary, local event logs, and installed
capture wiring stay on the tester's machine until explicitly removed.

## npx Wrapper

Planned v1 one-shot path:

```sh
npx vc-tokmeter --help
npx vc-tokmeter doctor
```

Use the wrapper when a tester does not want to keep a global executable. The
wrapper should invoke the same local commands as the installed binary; it should
not upload reports or telemetry.

Prerequisites:

- Node.js with `npx`.
- Provider-specific setup required by the selected adapter.

## pipx Wrapper

Planned v1 one-shot path:

```sh
pipx run vc-tokmeter --help
pipx run vc-tokmeter doctor
```

Use this route for Python-oriented environments that already standardize on
`pipx`. It is also a wrapper around local execution, not a hosted service.

Prerequisites:

- Python 3 with `pipx`.
- Provider-specific setup required by the selected adapter.

## Unsupported Until Packaged

The npm and pipx wrappers are placeholders until release packaging exists. Do
not ask external testers to rely on them before the corresponding package names
are published and `tokmeter doctor` passes through the wrapper path.
