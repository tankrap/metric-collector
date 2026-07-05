# Producing Comparison Numbers

Passive capture is the product path. Task mode is the lab path.

Use this guide only when a tester intentionally wants controlled
baseline/treatment numbers. The passive quickstart in the README is the
five-minute onboarding path; Mode T adds structure, repetition, and completion
checks so a report can make a controlled comparison claim.

## When To Use Mode T

Use Mode T when you need Grade P evidence:

- comparing a baseline workflow against a treatment workflow
- measuring tokens per completed task
- checking whether a treatment lowered token cost without lowering completion
  rate
- collecting medians and IQR across repeated task/profile runs

Do not use Mode T as the default product tour. A tester can get value from
passive Grade O reports without writing a task manifest.

## Mode T Flow

The source-checkout CLI can enter Mode T and plan task/profile runs:

```sh
cargo run -- run --profile baseline --task task-1
cargo run -- run --profile treatment --task task-1
cargo run -- run --next
```

The report layer can now produce Grade P comparison artifacts from two local
inputs:

- the core `events.jsonl` event log
- the versioned `completed-runs.tsv` run outcome store

The compare report joins records by `run_id`, `task_id`, and `profile_id`.
Token deltas use completed task records only. Failed and aborted records remain
visible in the side-by-side completion-rate table so a cheaper treatment cannot
hide lower task completion.

The public report helper is ready for CLI wiring. The remaining command-line
work is to expose a `report --compare --completed-runs <path>` style entry point
from `src/main.rs`.

Runs are grouped by profile:

- `baseline`: the tester's existing agent workflow.
- `treatment`: the same task set using the new stack or capture treatment.

Mode T uses a local `tasks.yaml` manifest with realistic tasks from the tester's
own repository. Each task has a one-line done condition. See
[Task manifests](tasks.md) and [examples/tasks.yaml](../examples/tasks.yaml) for
the manifest format.

## Evidence Grades

Reports must label evidence explicitly.

Grade O is observational passive data. It can show what happened during normal
work: operation mix, token estimates, event counts, and descriptive deltas
between time windows. It must not present an observational delta as a savings
claim because the work was not controlled.

Grade P is controlled Mode T data. It can compare tokens per completed task
between baseline and treatment profiles. Grade P comparisons use completed task
runs only, show completion rates next to token totals, and show medians and IQR
when repetitions exist.

A treatment that appears cheaper by failing tasks must be visible as a lower
completion rate, not hidden in the math.

## Repetition And Dispersion

TOPEN-3 is scoped to Mode T only. Passive Grade O reports do not need repeated
task runs or dispersion statistics to be useful as local self-reports.

Mode T comparisons require at least two repetitions per task/profile. Reports
show medians and IQR when repeated measurements exist, and they always display
completion rates next to token totals.

Small samples are expected during early tester runs. The goal is to surface
uncertainty without turning the passive product path into a benchmark campaign.

## Privacy

Task manifests and comparison reports can be shared manually, so keep task text
generic. Do not include source snippets, stack traces, internal URLs, customer
names, secrets, proprietary feature names, exact file paths, branch names, or
private issue links.

`vc-tokmeter report --share` writes an explicit manual share artifact. For
comparison reports, sharing should happen from the generated Grade P
`report.json` and `report.md` so the same completion-rate warnings travel with
the numbers.
