# Privacy and Safety

vc-tokmeter is intended to run beside private codebases. The implementation
should preserve tester trust before it optimizes for richer telemetry.

## Local Data Model

Adapters must not persist source code, prompt text, file contents, or raw tool
outputs. They may process content in memory long enough to compute:

- byte counts
- token counts or estimates
- content digests
- operation classes
- timing and run metadata

Persisted event logs should contain only privacy-safe measurement fields.
Current regression tests scan fixture logs for known secret, prompt, source,
path, and tool-output markers so accidental raw-content persistence fails in
CI.

## Paths and Sharing

Local reports may use clear paths when that helps the tester understand their
own data. Shared reports must replace paths and repository names with salted
hashes and truncate digests.

`report --share` is the only local artifact testers should be asked to send
manually. Sharing is explicit and manual. A share artifact may include
aggregate token counts, operation classes, completion rates, warning labels,
salted path/repository hashes, and truncated digests. It must not include
prompts, source snippets, raw tool output, provider credentials, or unhashed
private paths.

Hosted collection, when implemented, must use the same redacted boundary. It is
allowed only as an explicit opt-in upload of the aggregate payload documented in
[`upload-schema.md`](upload-schema.md). The upload payload is not a transport
for raw event logs, transcripts, prompts, source, tool output, exact paths,
branch names, provider requests/responses, or credentials.

Grade P comparison shares use the same redaction path as passive reports. They
may include baseline/treatment profile labels, completed-task token totals,
tokens per completed task, and side-by-side completion rates. They must not add
task prompts, done-condition text, local completion notes, or raw event payloads.

Passive reports are Grade O evidence: observational workloads were not
controlled, so differences may reflect changes in the work itself. Grade O
artifacts can show descriptive deltas, but they must not present savings claims.
Savings claims require Grade P evidence from the controlled Mode T comparison
protocol.

## Network Behavior

Local measurement has no telemetry phone-home. The proxy adapter may forward the
user's own provider traffic when explicitly configured, but tokmeter itself
should not send measurement data to a project-owned service unless the tester
chooses an explicit upload command.

There must be no automatic upload, background synchronization, hidden anonymous
aggregation, or opt-out analytics path. Hosted aggregation is opt-in only. The
first upload flow must show what is being sent, identify the schema version and
study, and require affirmative consent before network transfer.

`vc-tokmeter setup` defaults to local-only reporting. It saves upload endpoint
and token values only when a tester provides both upload enrollment flags. The
saved `.tokmeter/upload.json` file can be removed with
`vc-tokmeter setup --remove-upload-config`. `vc-tokmeter upload` produces a
dry-run payload review unless the tester passes `--yes`.

Opt-in upload must remain separate from `report --share`: share creates a local
redacted artifact, while upload sends a versioned aggregate payload to the
configured collector. Disabling upload or uninstalling tokmeter must leave local
reporting intact.

## Uploaded Data Retention

The default hosted retention target is 180 days for accepted aggregate upload
payloads and 30 days for server access logs, unless a published study protocol
uses a shorter window. Testers should be able to request deletion by tester
alias, upload ID, or session hash when they can provide one of those
identifiers.

Deletion should remove the upload payload and derived dashboard rows that can
still be linked to that payload. Published aggregate summaries may remain only
when they can no longer be traced back to an individual tester or upload.

## Credentials

The proxy adapter handles provider credentials as pass-through data. It must not
log authorization headers, API keys, prompts, responses, or error payloads that
contain secrets.

Credential-bearing upstream URLs are rejected by setup metadata helpers. Error
messages and diagnostics should redact authorization headers, API-key-like
values, bearer tokens, and provider response bodies before they are printed or
persisted.

Upload setup and upload rendering must identify whether a token came from a
flag or config file without printing the token value itself.

## Uninstall

`tokmeter init` should print every local change it makes. `tokmeter uninstall`
should remove tokmeter-created hooks and configuration without modifying
unrelated user settings.

Uninstall is part of the trust model: every installed hook, config entry, and
log pointer should have a matching removal instruction, and partial setup
failures should leave enough metadata for cleanup.
