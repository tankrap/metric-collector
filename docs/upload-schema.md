# Opt-in Upload Schema

This document defines the v1 hosted collection contract for testers who choose
to contribute aggregate vc-tokmeter measurements. Upload is a separate,
explicit action. Local measurement, report generation, and `report --share`
continue to work without contacting a tokmeter-owned service.

The normative machine-readable schema is
[`schemas/upload-payload-v1.schema.json`](../schemas/upload-payload-v1.schema.json).

## Contract

Upload payloads use `schema_version: "vc-tokmeter.upload.v1"` and
`artifact_type: "vc-tokmeter.upload"`.

The payload is an envelope around aggregate, redacted report data. It is based
on the same privacy boundary as `report --share`, not on raw
`.tokmeter/events.jsonl` records. The collector must reject raw event logs,
provider transcripts, prompts, tool input/output, source snippets, exact paths,
branch names, credentials, and arbitrary provider request/response bodies.

Client enrollment is opt-in. Default setup is local-only; setup saves
`.tokmeter/upload.json` only when both endpoint and token are explicitly
provided, and `vc-tokmeter setup --remove-upload-config` removes the saved
endpoint/token. The first upload step should be `vc-tokmeter upload --dry-run`
so the tester can inspect `upload.payload.json`; network transfer requires
`vc-tokmeter upload --yes`.

Required top-level sections:

- `client`: tokmeter version, measured surface, and coarse platform.
- `consent`: explicit upload opt-in state, consent version, optional tester
  alias, and optional contact permission.
- `study`: study identifier, protocol version, and optional cohort.
- `session`: hashed session/repository identifiers, coarse UTC hour bucket,
  duration, and optional timezone offset.
- `metrics`: aggregate report metrics, token fidelity, token source breakdown,
  session git share, and git workflow aggregate rows.
- `redaction`: declares that the payload came from the report-share boundary
  and follows the aggregate-only private-data policy.

## Allowed Metadata

Allowed metadata is intentionally narrow:

- Tool surface: `codex-cli`, `codex-tui`, `codex-exec`, `claude-code`,
  `claude-desktop`, `mcp-desktop`, or `other`.
- Platform: operating system and CPU architecture only.
- Version: vc-tokmeter version and study protocol version.
- Time: coarse UTC hour bucket plus duration; exact local event timestamps stay
  local.
- Identity: optional tester alias chosen for the study, plus salted hashes for
  session and repository identifiers.
- Metrics: aggregate token counts, byte counts, event counts, evidence grade,
  fidelity label, source buckets, and git action subtype summaries.

## Forbidden Data

Upload payloads must not contain:

- Prompts, chat messages, provider transcripts, or completion text.
- Source code, file contents, raw diffs, or raw tool output.
- Provider credentials, authorization headers, cookies, API keys, or upstream
  URLs containing credentials.
- Exact file paths, repository names, directory names, or branch names.
- Raw `.tokmeter/events.jsonl` records or provider request/response payloads.
- Full content digests that can be used as durable cross-study identifiers.

Hashed identifiers are allowed only when they are salted locally and scoped to
the tester or session. The v1 upload schema does not include branch hashes; the
study does not need branch-level linkage.

## Example

See
[`server/collector/tests/fixtures/upload-payload-v1.valid.json`](../server/collector/tests/fixtures/upload-payload-v1.valid.json)
for a valid payload fixture.
The collector regression suite also includes
[`server/collector/tests/fixtures/upload-payload-v1.forbidden-field.json`](../server/collector/tests/fixtures/upload-payload-v1.forbidden-field.json)
to ensure forbidden fields fail validation.

## Retention and Deletion

The hosted collector should retain accepted aggregate upload payloads for up to
180 days by default, unless a published study protocol states a shorter period.
Server access logs should be retained for no more than 30 days.

Deletion requests should be possible by tester alias, upload ID, or session hash
when a tester can provide one of those identifiers. Deletion removes the upload
payload and any derived dashboard aggregate rows that can still be linked back
to that payload. Fully anonymous published summaries may remain if they can no
longer be traced to an individual upload.

## Compatibility

Any incompatible change must create a new schema version. Additive optional
fields may be introduced only when they do not weaken the forbidden-data policy.
The collector should store the original schema version with every accepted
payload so old study data remains interpretable.
