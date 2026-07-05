# Claude Code collection

Use `claude-code` when you want a long interactive Claude Code session with
tokmeter collection running for the whole session.

Run this from the repository being measured:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- claude-code
```

The wrapper starts a localhost-only proxy, sets `ANTHROPIC_BASE_URL` for the
launched `claude` process, appends proxy events to `.tokmeter/events.jsonl`, and
writes `.tokmeter/report` after Claude exits. It does not set or persist
provider credentials.

While Claude Code is still running, generate an interim report from another
terminal:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- report \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

Pass Claude Code arguments after `--`:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- claude-code -- \
  --model sonnet
```

By default, the wrapper removes `ANTHROPIC_API_KEY` from the launched Claude
process so an existing shell API key does not accidentally override a logged-in
Claude subscription session. If you intentionally want API-key mode, keep the
key:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- claude-code \
  --keep-anthropic-api-key
```

Use a custom Claude binary, port, upstream, event log, or report directory when
needed:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- claude-code \
  --claude-bin /path/to/claude \
  --port 17684 \
  --upstream https://api.anthropic.com \
  --event-log .tokmeter/events.jsonl \
  --out .tokmeter/report
```

## What gets captured

The proxy path captures provider usage fields when Anthropic returns them, and
uses estimated token-equivalent volume when a response does not expose usage.
Exact Anthropic usage events are labeled with the `proxy.claude.anthropic`
adapter. Estimated fallback events keep the existing `proxy.estimated` label so
reports continue to mark them as estimates.

For git-specific attribution, also run `init` in the measured repository so
Claude Code hooks record tool inputs and outputs:

```sh
cargo run --manifest-path /Users/justin/metrics/Cargo.toml -- init
```

Then launch the wrapped session from that same repository. Reports combine the
hook events and proxy token events from the shared `.tokmeter/events.jsonl`.

## Caveats

Claude Code must support `ANTHROPIC_BASE_URL` for the current authentication
mode on the tester's machine. API-key mode is the most direct proxy path.
Logged-in subscription sessions should be tested locally because Claude Code may
apply additional restrictions when requests are routed through a non-first-party
host.

The proxy is intentionally localhost-only. It forwards credentials to the
configured upstream but does not persist credentials, prompts, response content,
source code, or raw tool output.
