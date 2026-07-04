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

`report --share` is the only artifact testers should be asked to send. Sharing
is explicit and manual. A share artifact may include aggregate token counts,
operation classes, completion rates, warning labels, salted path/repository
hashes, and truncated digests. It must not include prompts, source snippets,
raw tool output, provider credentials, or unhashed private paths.

Passive reports are Grade O evidence: observational workloads were not
controlled, so differences may reflect changes in the work itself. Grade O
artifacts can show descriptive deltas, but they must not present savings claims.
Savings claims require Grade P evidence from the controlled Mode T comparison
protocol.

## Network Behavior

v1 has no telemetry phone-home. The proxy adapter may forward the user's own
provider traffic when explicitly configured, but tokmeter itself should not send
measurement data to a project-owned service.

There is no automatic upload, background synchronization, anonymous
aggregation, or opt-out analytics path in v1. Future aggregation, if any, must
be a separate opt-in feature and should not change the meaning of
`report --share`.

## Credentials

The proxy adapter handles provider credentials as pass-through data. It must not
log authorization headers, API keys, prompts, responses, or error payloads that
contain secrets.

Credential-bearing upstream URLs are rejected by setup metadata helpers. Error
messages and diagnostics should redact authorization headers, API-key-like
values, bearer tokens, and provider response bodies before they are printed or
persisted.

## Uninstall

`tokmeter init` should print every local change it makes. `tokmeter uninstall`
should remove tokmeter-created hooks and configuration without modifying
unrelated user settings.

Uninstall is part of the trust model: every installed hook, config entry, and
log pointer should have a matching removal instruction, and partial setup
failures should leave enough metadata for cleanup.
