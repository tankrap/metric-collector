# Distribution and Wrapper Commands

vc-tokmeter is designed to support installed binaries and one-shot wrapper
commands. This repository is still in early implementation, so the source
checkout path is the currently usable route. The other commands below are the
planned v1 user-facing commands and should not be published as working install
instructions until packaging is wired.

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
- Claude Code hooks or local proxy wiring once the matching adapter is enabled.

## Static Binary

Planned v1 install path:

```sh
curl -L https://github.com/tankrap/metric-collector/releases/latest/download/vc-tokmeter-macos-arm64.tar.gz -o vc-tokmeter.tar.gz
tar -xzf vc-tokmeter.tar.gz
chmod +x vc-tokmeter
./vc-tokmeter --help
```

Installed binary usage:

```sh
./vc-tokmeter init
./vc-tokmeter doctor
./vc-tokmeter run --profile baseline --task task-id
./vc-tokmeter report --compare
```

This path is for repeat local use. The binary, local event logs, and installed
capture wiring stay on the tester's machine until explicitly removed.

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

## Unsupported Until Packaged

The npm and pipx wrappers are placeholders until release packaging exists. Do
not ask external testers to rely on them before the corresponding package names
are published and `tokmeter doctor` passes through the wrapper path.
