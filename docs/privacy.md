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

## Paths and Sharing

Local reports may use clear paths when that helps the tester understand their
own data. Shared reports must replace paths and repository names with salted
hashes and truncate digests.

`report --share` is the only artifact testers should be asked to send. Sharing
is explicit and manual.

## Network Behavior

v1 has no telemetry phone-home. The proxy adapter may forward the user's own
provider traffic when explicitly configured, but tokmeter itself should not send
measurement data to a project-owned service.

## Credentials

The proxy adapter handles provider credentials as pass-through data. It must not
log authorization headers, API keys, prompts, responses, or error payloads that
contain secrets.

## Uninstall

`tokmeter init` should print every local change it makes. `tokmeter uninstall`
should remove tokmeter-created hooks and configuration without modifying
unrelated user settings.
