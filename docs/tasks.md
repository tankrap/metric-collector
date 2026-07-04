# Task Manifests

Task manifests define the small set of work items used for baseline and
treatment comparisons. Start by copying `examples/tasks.yaml` into your working
area, then replace each sample task with a real task from the repository you are
measuring.

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

Use 5 to 15 tasks per comparison. Keep descriptions to one line because the
current parser accepts only single-line scalar values.

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
