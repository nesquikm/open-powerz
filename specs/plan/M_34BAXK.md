---
milestone: M_34BAXK
id: fr_01M5CXT39J3QY24WCCV734BAXK
status: active
archived_at: null
kickoff_branch: null
frozen_at: null
migration: none
codename: Wire Whisperer
---

# Implementation Plan

## M_34BAXK — Protocol Core and CLI {#M_34BAXK}

**Goal:** A tested Rust protocol library that talks to a KM003C / KM002C over vendor bulk (CDC as second pipe, HID as a read-only ADC fallback), reads measurements, settings and device info, authenticates with the shipped keys and streams the ADC queue at 2/10/50/1000 SPS — plus the `powerz` CLI on top and the in-repo protocol reference.
**Prerequisites:** Bootstrap (done, below).

**FR list:**

| FR | Title | Tracker |
|----|-------|---------|
| [ZVJ3EB](../frs/ZVJ3EB.md) | Protocol reference document | n/a |
| [BXYTJX](../frs/BXYTJX.md) | Frame codec | n/a |
| [E88WR5](../frs/E88WR5.md) | Measurement and settings decoders | n/a |
| [SJGK7E](../frs/SJGK7E.md) | Device discovery and transports | n/a |
| [N6EHQC](../frs/N6EHQC.md) | Device session: auth, device info and streaming | n/a |
| [STQEA1](../frs/STQEA1.md) | powerz command-line tool | n/a |

**Tasks:**
- [x] Cargo workspace with `crates/powerz-protocol`, `crates/powerz-cli`, `apps/desktop/src-tauri`
  verify: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [x] Svelte 5 + Vite 8 + TS web UI with Vitest, ESLint, Prettier, svelte-check
  verify: `bun run lint && bun run check && bun run test && bun run build:web`
- [x] GitHub Actions gate on ubuntu-24.04, macos-latest, windows-latest
  verify: `.github/workflows/ci.yml` runs the gating rule
- [ ] Raise the workspace MSRV to Rust 1.96 and refresh the lockfile (unpins 34 transitive crates)
  verify: `cargo update --workspace` then the gating rule passes
- [ ] Protocol reference document — ZVJ3EB
  verify: `cargo test -p powerz-protocol --test protocol_doc`
- [ ] Frame codec — BXYTJX
  verify: `cargo test -p powerz-protocol frame`
- [ ] Measurement and settings decoders — E88WR5
  verify: `cargo test -p powerz-protocol decode`
- [ ] Device discovery and transports — SJGK7E
  verify: `cargo test -p powerz-protocol transport`
- [ ] Device session: auth, device info and streaming — N6EHQC
  verify: `cargo test -p powerz-protocol --test session_stream`
- [ ] powerz command-line tool — STQEA1
  verify: `cargo test -p powerz-cli`
- [ ] Real-device smoke run of every `powerz` subcommand on the KM003C over bulk and CDC (manual)
  verify: transcript of `powerz list/info/read/stream --rate 1000` attached to the PR

**Tests:**
- Byte-vector unit tests from session-1 captures and hand-decoded vendor samples; `proptest` no-panic properties for every parser; scripted `MockTransport` sessions; CLI command functions against the mock; protocol-doc structure test.

**Acceptance Criteria:**
- [x] Gating rule from CLAUDE.md passes locally on macOS
- [ ] CI is green on all three OSes
- [ ] Every AC of the six FRs above is traced to a test by `/spec-review`
- [ ] `powerz stream --rate 1000` records 60 s on the KM003C with no host-side gaps

**Gate:** `bun install --frozen-lockfile && bun run lint && bun run check && bun run test && bun run build:web && cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
