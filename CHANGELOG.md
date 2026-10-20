# Changelog

All notable changes to open-powerz are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] — 2026-10-20 — "Wire Whisperer"

### Added
- Protocol reference document: `docs/protocol.md`, the KM003C / KM002C USB protocol in our own words, with confidence tags (ZVJ3EB)
- Frame codec: message headers, PUT_DATA section walker and sequence counter (BXYTJX)
- Measurement and settings decoders: ADC snapshot, ADC queue records, sample rates, settings and device-info blocks (E88WR5)
- Device discovery and transports: vendor bulk (`nusb`), CDC (`serialport`) and a guarded, ADC-only HID fallback (`hidapi`) (SJGK7E)
- Device session: CONNECT, memory read, AES authentication with the shipped keys, device info and ADC-queue streaming at 2/10/50/1000 SPS (N6EHQC)
- `powerz` command-line tool: `list`, `info`, `read`, `watch` and `stream` (vendor-compatible CSV), `--json`, `--trace` and stable exit codes (STQEA1)
- Device simulator (the demo device, `--transport sim`) with named scenarios, and end-to-end CLI tests against it (G0W08Z)

Total test count at release: 370 tests, 0 failures, 0 errors.
