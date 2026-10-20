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
| `crates/powerz-cli` | `powerz` command-line tool |
| `apps/desktop` | Tauri 2 desktop app (Svelte 5 + TypeScript + Vite, uPlot charts) |
| `specs/` | requirements, technical spec, plans |

The USB protocol the meter speaks is documented in [docs/protocol.md](docs/protocol.md).

## Building

Requirements: Rust (stable, 1.96 or newer), [Bun](https://bun.sh), and the
[Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```bash
bun install
bun run build:web             # build the web UI (the Tauri crate embeds it)
cargo build --workspace       # library, CLI and desktop app
cargo run -p powerz-cli       # the CLI
bun run dev                   # desktop app with hot reload
```

## License

MIT, see [LICENSE](LICENSE). The repository will also ship the AES keys the meter uses
for authenticated features (queue streaming, device info, offline records), so those
work without extra setup.
