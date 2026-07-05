# Tester Invitation Template

Subject: Metric Taker opt-in study invitation

Hi <tester name>,

We are inviting a small group of testers to try Metric Taker and optionally
upload aggregate token metrics from a version-control workflow session.

The upload is opt-in. It sends only the `vc-tokmeter.upload.v1` aggregate
payload: token totals, git-token share, surface, coarse session metadata, and
redaction metadata. It must not include prompts, messages, source code, command
output, file paths, branch names, repository names, credentials, or API keys.

Before uploading, please run the local report/share flow and inspect the
redacted artifact. Upload only if you are comfortable sharing the aggregate
payload.

Study settings:

```sh
export VC_TOKMETER_UPLOAD_ENDPOINT="<collector-url>/v1/uploads"
export VC_TOKMETER_UPLOAD_TOKEN="<upload-token>"
```

Suggested check:

```sh
vc-tokmeter doctor
vc-tokmeter report --share
```

When you are ready to submit:

```sh
vc-tokmeter upload \
  --endpoint "$VC_TOKMETER_UPLOAD_ENDPOINT" \
  --token "$VC_TOKMETER_UPLOAD_TOKEN"
```

Please send back:

- The upload timestamp.
- Your tester alias, if you used one.
- Any warning output from `vc-tokmeter upload`.
- Whether the session used Codex TUI, Claude Code, or another surface.

Do not send raw prompts, transcripts, repository files, diffs, branch names, or
API keys in your reply.

If you want your upload removed later, contact <study contact> with your tester
alias, upload ID, or session hash if you have it.
