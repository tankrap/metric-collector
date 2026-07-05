# Task Manifests

Task manifests are used only by Mode T, the lab path for producing controlled
comparison numbers. They are not required for the passive product quickstart.

Start with [Producing comparison numbers](producing-comparison-numbers.md) when
you need baseline/treatment evidence. Then copy `examples/tasks.yaml` into your
working area and replace each sample task with a real task from the repository
you are measuring.

Keep the shape of the file unchanged:

```yaml
tasks:
  - id: task-1
    title: Short task title
    description: Done when one observable condition is true.
    done: false
```

Each task needs:

- `id`: a stable, unique identifier for the task.
- `title`: a short repo-agnostic summary of the work.
- `description`: a one-line done condition that says how to recognize
  completion.
- `done`: `false` before the run, then `true` only after the task is complete.

Use 5 to 15 tasks per Mode T comparison. Keep descriptions to one line because
the current parser accepts only single-line scalar values.

## Run Progress Records

Mode T run progress is stored as `completed-runs.tsv`. The file is versioned and
contains only public run metadata: run ID, task ID, profile, repetition,
completion status, adapter, and timing fields.

`run --next` scheduling should use only records whose status is `completed`.
Failed or aborted records remain useful for report math, but they do not advance
the interleaved baseline/treatment scheduler.

Local completion notes are never written to this shared progress file. Keep
private debugging context in local notes only, not in task titles or done
conditions.

The run wrapper API plans from the persisted store, executes a supplied command
or session closure, accepts a completion decision, and appends the public record.
The CLI integration should wire `tokmeter run --next -- <agent command...>` to
`execute_wrapped_run(...)`, pass the real completion prompt decision, and use
`completion_decision_from_command_outcome(...)` as the non-interactive default.
Only `completed` records should be fed back into scheduler planning.

## Writing Done Conditions

Write done conditions as observable outcomes, not implementation notes. Good
conditions usually start with `Done when` and describe the result a reviewer or
test can check.

Examples:

- `Done when the empty state shows the expected message.`
- `Done when invalid input has a focused regression test.`
- `Done when stale setup notes are removed from the config guide.`

Avoid vague conditions such as `Done when the code is improved` or `Done when it
works`.

## Sharing Safely

Task manifests may be copied into reports or shared artifacts, so keep them free
of private codebase details. Do not include source snippets, stack traces,
internal URLs, customer names, secrets, proprietary feature names, exact file
paths, branch names, or issue links.

Prefer generic wording that preserves the kind of work without exposing the
repository:

- Use `Fix incorrect empty-state message` instead of naming the private screen.
- Use `Simplify duplicated validation logic` instead of naming internal modules.
- Use `Clean up stale configuration notes` instead of linking private docs.
