# vc-tokmeter

vc-tokmeter is a local measurement harness for estimating how many LLM context
tokens an agent workflow spends on version-control and file interaction.

The design goal is simple: a tester should get from install to first local
numbers in under five minutes, without sending source code, prompts, tool
outputs, or file contents anywhere.

## Status

This repository is in early implementation. The source-checkout CLI can run the
passive quickstart, produce first local Grade O report artifacts, enter Mode T
run planning, run `doctor`, and remove tokmeter-created setup files. Full
adapter capture, packaged installs, compare reports, and share export are still
v1 work in progress.

## Passive Quickstart

From a checked-out copy of this repository:

```sh
cargo run -- --help
```

Install options are documented in [docs/distribution.md](docs/distribution.md).
Today the source checkout path is the only implemented path. Static binary,
`npx`, and `pipx` wrappers are v1 distribution targets and are documented with
their intended commands so tester instructions can stabilize before packaging
lands.

Passive-first source-checkout flow:

```sh
cargo run -- init
cargo run -- status
cargo run -- report --out .tokmeter/report
cargo run -- doctor
cargo run -- uninstall
```

For Codex CLI testing, run `init` from the repository you want to measure. From
a source checkout, use this form from the target repository:

```sh
cargo run --manifest-path /path/to/metric-collector/Cargo.toml -- init
```

It writes a project-local `.codex/hooks.json` with `PreToolUse` and
`PostToolUse` command hooks that append privacy-safe events to
`.tokmeter/events.jsonl`. Start Codex in that trusted project, open `/hooks`,
trust the new tokmeter hooks if prompted, then use Codex normally. Generate a
local report with:

```sh
cargo run --manifest-path /path/to/metric-collector/Cargo.toml -- report \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

Passive mode is the product path. The five-minute setup test measures this
path: install, initialize local capture, check status, and produce the first
local report. Current source-checkout reports use Grade O self-report fixture
data until full adapter event aggregation is wired into the CLI.

To verify the source-checkout onboarding path from a clean temporary copy:

```sh
scripts/timed-onboarding-smoke.sh
```

`vc-tokmeter status` is the lightweight self-report command. In the current
early CLI it prints:

```text
mode=passive task_id=adhoc profile=adhoc events_today=0 top_op_class=n/a
```

That line is intentionally safe to paste into an issue or chat. It reports
mode, task/profile labels, today's local event count, and the top operation
class without source, prompt, path, credential, or tool-output content.

`init` installs local passive capture wiring, prints exactly what changed and
how to remove it, and includes a one-line pointer to optional task mode.
`doctor` verifies the local wiring path with a short self-test. `report --share`
is the planned redacted export path that a tester can manually choose to send.

## Producing Comparison Numbers

Task mode is not the product quickstart. It is Mode T, the lab path for
controlled baseline/treatment numbers. Use
[Producing comparison numbers](docs/producing-comparison-numbers.md) only when a
tester intentionally wants Grade P evidence from repeated task runs. The current
CLI can plan and stamp Mode T runs; full compare aggregation remains in progress.

## Privacy

vc-tokmeter is designed to be local-first:

- No source code, prompt text, file contents, or raw tool outputs are persisted
  by adapters.
- Event logs store counts, byte sizes, digests, operation classes, timestamps,
  and run metadata.
- There is no telemetry phone-home in v1.
- Sharing is manual through `vc-tokmeter report --share`.

See [docs/privacy.md](docs/privacy.md) for the detailed trust model.

## Development

```sh
cargo fmt
cargo test
```

The current implementation intentionally avoids third-party dependencies while
the core shapes settle.
