# open-powerz

An open-source, cross-platform (macOS, Linux, Windows) desktop app and CLI for the
ChargerLAB **POWER-Z KM003C / KM002C** USB-C power meter and PD analyzer. It aims to
cover the features of the vendor's Windows-only "POWER-Z Lab" PC software with a
modern UI.

> **Unofficial project.** open-powerz is not made, endorsed or supported by ChargerLAB.
> "POWER-Z" and "ChargerLAB" are trademarks of their owner. For the official software,
> manuals and firmware, see the
> [POWER-Z technical support page](https://www.power-z.com/blogs/technical-support/power-z-km003c-km002c-technical-support).

**Status: early development.** Nothing talks to a device yet.

## Planned features

- Live meter: VBUS, IBUS, power, D+/D−, CC1/CC2, temperature
- Recorder at up to 1000 samples/s with charts, statistics and CSV export
- USB PD / UFCS sniffer with a PD 3.2 decoder
- Protocol triggering over the meter's virtual serial port
- A CLI for scripting and raw protocol dumps

## Layout

| path | what |
|---|---|
| `crates/powerz-protocol` | Rust protocol library: transports (vendor bulk, HID, CDC), framing, decoders |
| `crates/powerz-record` | vendor-compatible CSV recorder shared by the CLI and the desktop app |
| `crates/powerz-cli` | `powerz` command-line tool |
| `apps/desktop` | Tauri 2 desktop app (Svelte 5 + TypeScript + Vite, uPlot charts) |
| `specs/` | requirements, technical spec, plans |

The USB protocol the meter speaks is documented in [docs/protocol.md](docs/protocol.md).

## Building

Requirements: Rust from [rustup](https://rustup.rs), [Bun](https://bun.sh), and the
[Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.
rustup installs the exact Rust version pinned in `rust-toolchain.toml` at the repo
root. The minimum supported Rust stays 1.96 (`rust-version` in `Cargo.toml`).

```bash
bun install
bun run build:web             # build the web UI (the Tauri crate embeds it)
cargo build --workspace       # library, CLI and desktop app
cargo run -p powerz-cli       # the CLI
bun run dev                   # desktop app with hot reload
```

### Rust toolchain

The Rust version is pinned in one place: `rust-toolchain.toml`. CI and release
builds use the same file and never name their own version; the gate fails if a
workflow does.

To move to a newer Rust:

1. Edit `channel` in `rust-toolchain.toml`.
2. Fix any new clippy lints and `cargo fmt` changes.
3. Open a pull request.

A Homebrew-installed `rust` (cargo not managed by rustup) ignores
`rust-toolchain.toml`. Put rustup's proxies first on your `PATH` instead, for
example `brew install rustup` and add `$(brew --prefix rustup)/bin` before
Homebrew's `bin`.

## Try it without a meter

The `powerz` CLI ships a simulated KM003C, the **demo device**:

```bash
cargo run -p powerz-cli -- --transport sim info
cargo run -p powerz-cli -- --transport sim --scenario load-5v-1a read
cargo run -p powerz-cli -- list --include-demo
```

Scenarios: `idle`, `load-5v-1a`, `pd-9v-2a`, `noisy-negative`. For tests, the
simulator also reads `POWERZ_SIM_CLOCK=fake` (deterministic simulated time),
`POWERZ_SIM_FAULT` (e.g. `timeout@2,disconnect@3,wrong-id@4,permission`) and
`POWERZ_SIM_CTRL_C_AFTER=N` (act as if Ctrl-C was pressed after N polls).

Hardware-in-the-loop tests against a real meter run only with `POWERZ_HIL=1`:

```bash
POWERZ_HIL=1 cargo test -p powerz-cli --test hil
```

## License

MIT, see [LICENSE](LICENSE). The repository will also ship the AES keys the meter uses
for authenticated features (queue streaming, device info, offline records), so those
work without extra setup.
