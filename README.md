# vc-tokmeter

vc-tokmeter is a local measurement harness for estimating how many LLM context
tokens an agent workflow spends on version-control and file interaction.

The design goal is simple: a tester should get from install to first local
numbers in under five minutes, without sending source code, prompts, tool
outputs, or file contents anywhere.

## Status

This repository is in early implementation. The CLI command names are reserved,
and the core modules are being built behind them. Until the adapters and report
pipeline are complete, commands other than `--help` may return a clear
not-implemented error.

## Quickstart

From a checked-out copy of this repository:

```sh
cargo run -- --help
```

Install options are documented in [docs/distribution.md](docs/distribution.md).
Today the source checkout path is the only implemented path. Static binary,
`npx`, and `pipx` wrappers are v1 distribution targets and are documented with
their intended commands so tester instructions can stabilize before packaging
lands.

Planned passive-first v1 flow:

```sh
vc-tokmeter init
vc-tokmeter status
vc-tokmeter report
vc-tokmeter report --share
vc-tokmeter uninstall
```

Passive mode is the product path: after `init`, capture is local and automatic
for supported adapters. `status` shows the current mode, events captured today,
and the top operation class. Task mode is optional lab machinery for controlled
comparison numbers.

`init` will detect supported agent tooling, install local passive capture
wiring, print exactly what changed and how to remove it, and include a one-line
pointer to optional task mode. `doctor` will verify the wiring and run a short
self-test. `report --share` will emit a redacted artifact that a tester can
manually choose to send.

## Producing Comparison Numbers

The comparison protocol is Mode T, the lab path for baseline/treatment numbers.
It uses a local `tasks.yaml` manifest with realistic tasks from the tester's own
repository. Each task has a one-line done condition. See
[docs/tasks.md](docs/tasks.md) and [examples/tasks.yaml](examples/tasks.yaml) for
a starter template.

Planned Mode T flow:

```sh
vc-tokmeter run --profile baseline --task task-id
vc-tokmeter run --profile treatment --task task-id
vc-tokmeter report --compare
```

Runs are grouped by profile:

- `baseline`: the tester's existing agent workflow.
- `treatment`: the same task set using the new stack or capture treatment.

Reports label evidence explicitly:

- Grade O is observational passive data. It can show descriptive before/after
  deltas but not savings claims.
- Grade P is controlled Mode T data. It can show tokens-per-completed-task
  comparisons from completed task runs only.

A treatment that appears cheaper by failing tasks should be visible as a lower
completion rate, not hidden in the math.

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
