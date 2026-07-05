# Distribution and Wrapper Commands

vc-tokmeter supports source-checkout development and installed release
binaries. Release artifacts are published for macOS and Linux so external
testers do not need Rust or a repository checkout.

## Release Artifacts

Tagged releases publish these tarballs:

- `vc-tokmeter-macos-arm64.tar.gz`
- `vc-tokmeter-macos-x64.tar.gz`
- `vc-tokmeter-linux-arm64.tar.gz`
- `vc-tokmeter-linux-x64.tar.gz`

Each stable installer tarball also has a versioned companion named
`vc-tokmeter-v<VERSION>-<platform>.tar.gz`. The release includes `SHA256SUMS`
for installer verification and individual `.sha256` files for each tarball.

Install the latest release with checksum verification:

```sh
curl -fsSL https://raw.githubusercontent.com/tankrap/metric-collector/main/install.sh | sh
```

By default this installs to `~/.local/bin`. To install elsewhere:

```sh
curl -fsSL https://raw.githubusercontent.com/tankrap/metric-collector/main/install.sh | sh -s -- \
  --prefix /usr/local
```

The installer verifies the tarball checksum before copying `vc-tokmeter`. By
default it also installs `codex` and `claude` shims in the same directory so
normal agent commands run through tokmeter collection. It prints PATH guidance
when the install directory is not already on `PATH` and does not edit shell
profiles.

## Hosted Bash Installer

The supported non-Rust install path is the GitHub-hosted `install.sh` script.
It downloads release archives from GitHub Releases, verifies the matching
`SHA256SUMS` entry, installs the binary, and prints PATH guidance without
modifying shell profiles.

Useful options:

```sh
# Install a specific release tag.
curl -fsSL https://raw.githubusercontent.com/tankrap/metric-collector/main/install.sh | sh -s -- \
  --version v0.1.1

# Install from another GitHub repo, useful for forks.
curl -fsSL https://raw.githubusercontent.com/tankrap/metric-collector/main/install.sh | sh -s -- \
  --repo owner/repo

# Install from a local or self-hosted artifact directory for smoke tests.
./install.sh --base-url ./dist --prefix /tmp/vc-tokmeter-install

# Install only vc-tokmeter, without codex/claude command shims.
./install.sh --no-agent-aliases
```

The release workflow uploads stable platform artifact names and a combined
`SHA256SUMS`, which keeps the hosted installer URL stable across releases.

The `codex` and `claude` shims are ordinary executable scripts. They search
`PATH` for the real downstream agent outside the tokmeter install directory and
then run `vc-tokmeter codex-tui --codex-bin <real-codex>` or
`vc-tokmeter claude-code --claude-bin <real-claude>`. Existing foreign files
named `codex` or `claude` in the install directory are not replaced unless the
installer is run with `--force-agent-aliases`.

## Current Source Checkout

Use this path while developing or testing from the repository:

```sh
git clone git@github.com:tankrap/metric-collector.git
cd metric-collector
cargo run -- --help
cargo test
```

Prerequisites:

- Rust stable toolchain with Cargo.
- A local repository where the tester can run baseline and treatment tasks.
- Codex hooks, Claude Code hooks, or local proxy wiring once the matching
  adapter is enabled.

## Codex Project Hook

From the repository you want to measure, run the source-checkout binary's
`init` command. During development that usually means invoking this checkout
with Cargo from the target repository:

```sh
cargo run --manifest-path /path/to/metric-collector/Cargo.toml -- init
```

`init` creates a project-local `.codex/hooks.json` containing Codex
`PreToolUse` and `PostToolUse` command hooks. The hook command points back to
the running `vc-tokmeter` binary and appends privacy-safe event records to
`.tokmeter/events.jsonl`.

Start Codex in that trusted project, run `/hooks`, and trust the tokmeter hook
definitions if Codex marks them for review. Then use Codex normally and produce
a local report:

```sh
cargo run --manifest-path /path/to/metric-collector/Cargo.toml -- report \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

## Local Proxy Runtime

The proxy runtime is intentionally localhost-only. `ProxyConfig::new` accepts
`localhost`, `127.0.0.1`, or `::1` bind hosts and rejects wildcard or LAN
addresses before binding.

The runtime supports HTTP, HTTPS, and Codex WebSocket upstream forwarding. It
rewrites the request target to the configured upstream base path, forwards
provider credentials to the upstream, relays the upstream response bytes or
WebSocket frames to the client, and captures only sanitized metadata locally:

- request method and query-redacted path;
- sensitive headers with values replaced by `[REDACTED]`;
- provider usage/cache token fields from the upstream response body or
  WebSocket response frames;
- attribution-compatible proxy events and core event-log JSONL records.

Prompt bodies, response content, tool output text, query credential values, and
provider credentials are not persisted in proxy capture records. By default,
`tokmeter proxy` appends captured core event records to `.tokmeter/events.jsonl`.

To test Codex through the proxy against OpenAI with your normal Codex
subscription login:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- proxy \
  --bind-host 127.0.0.1 \
  --port 17683 \
  --upstream https://api.openai.com \
  --event-log .tokmeter/events.jsonl
```

Then start Codex in a separate terminal with a user-level base URL override:

```sh
codex -c openai_base_url='"http://127.0.0.1:17683/v1"'
```

Do not set a separate Platform API key for this subscription test. Codex will
send its normal cached Codex/ChatGPT credential through the local proxy. The
proxy upgrades local `ws://127.0.0.1:17683/v1/responses` requests to upstream
`wss://api.openai.com/v1/responses`.

Run a short Codex task, stop the proxy, and regenerate the report:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- report \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

## Claude Code Proxy Wrapper

For long Claude Code sessions, use the wrapper so the proxy and report path are
managed for the whole interactive session:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- claude-code
```

The wrapper starts the localhost proxy on an OS-assigned free port, sets
`ANTHROPIC_BASE_URL` for the launched `claude` process, removes
`ANTHROPIC_API_KEY` by default so subscription login is not accidentally
overridden, and writes a report after Claude exits. This allows multiple
tokmeter-wrapped Claude sessions to run at the same time. Use
`--keep-anthropic-api-key` for intentional API-key testing.

Pass Claude Code arguments after `--`:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- claude-code -- \
  --model sonnet
```

See [claude-code.md](claude-code.md) for full setup notes and caveats.

## Static Binary

Manual install path:

```sh
curl -L https://github.com/tankrap/metric-collector/releases/latest/download/vc-tokmeter-macos-arm64.tar.gz -o vc-tokmeter.tar.gz
tar -xzf vc-tokmeter.tar.gz
./vc-tokmeter-*/vc-tokmeter --help
```

Installed binary usage:

```sh
vc-tokmeter setup
vc-tokmeter doctor
vc-tokmeter live-test doctor --repo /path/to/repository
vc-tokmeter report --event-log .tokmeter/events.jsonl --out .tokmeter/report
```

This path is for repeat local use. The binary, local event logs, and installed
capture wiring stay on the tester's machine until explicitly removed.

Upload enrollment is not part of default setup. To opt in, save collector
credentials explicitly, review the redacted payload first, then confirm the
network upload:

```sh
vc-tokmeter setup --upload-endpoint https://collector.example.test/v1/uploads \
  --upload-token "$VC_TOKMETER_UPLOAD_TOKEN"
vc-tokmeter upload --dry-run
vc-tokmeter upload --yes
```

The saved upload config lives at `.tokmeter/upload.json`. Rendered setup and
upload output never prints the token value. To return to local-only reporting:

```sh
vc-tokmeter setup --remove-upload-config
```

## Uninstall and Cleanup

Run uninstall from the repository that was configured:

```sh
vc-tokmeter uninstall
vc-tokmeter doctor
```

Uninstall removes tokmeter-created hook blocks and setup files while preserving
unrelated config entries. It intentionally leaves local evidence in `.tokmeter`
so testers can inspect or upload aggregate reports later. To delete local event
logs and reports after the study is complete:

```sh
rm -rf .tokmeter
```

To remove the installed executable installed by `install.sh`, delete the binary
from the selected install directory:

```sh
rm -f "$HOME/.local/bin/vc-tokmeter"
```

## npx Wrapper

Planned v1 one-shot path:

```sh
npx vc-tokmeter --help
npx vc-tokmeter doctor
```

Use the wrapper when a tester does not want to keep a global executable. The
wrapper should invoke the same local commands as the installed binary; it should
not upload reports or telemetry.

Prerequisites:

- Node.js with `npx`.
- Provider-specific setup required by the selected adapter.

## pipx Wrapper

Planned v1 one-shot path:

```sh
pipx run vc-tokmeter --help
pipx run vc-tokmeter doctor
```

Use this route for Python-oriented environments that already standardize on
`pipx`. It is also a wrapper around local execution, not a hosted service.

Prerequisites:

- Python 3 with `pipx`.
- Provider-specific setup required by the selected adapter.

## Live Test Matrix

Installed users can print the same live-agent matrix without source scripts:

```sh
vc-tokmeter live-test doctor --repo /path/to/repository
vc-tokmeter live-test codex-exec --repo /path/to/repository
vc-tokmeter live-test codex-tui-api --repo /path/to/repository
vc-tokmeter live-test codex-tui-subscription --repo /path/to/repository
vc-tokmeter live-test claude-code-api --repo /path/to/repository
vc-tokmeter live-test claude-code-subscription --repo /path/to/repository
vc-tokmeter live-test claude-desktop-config --repo /path/to/repository
vc-tokmeter live-test report --repo /path/to/repository
```

Each command prints the event log path, report path, commands to run, and key
summary lines to inspect in the generated report.

## Release Smoke Tests

Release CI packages each target, installs the stable tarball through
`install.sh`, verifies the checksum path, runs `vc-tokmeter --help`, `doctor`,
`setup`, `report`, and `uninstall` from a temporary target repository, and
checks that uninstall removed tokmeter hooks without deleting unrelated config.
Run the same smoke locally after packaging:

```sh
scripts/release-install-smoke.sh --artifact dist/vc-tokmeter-macos-arm64.tar.gz
```

## Unsupported Until Packaged

The npm and pipx wrappers are placeholders until those package names are
published and `vc-tokmeter doctor` passes through each wrapper path.
