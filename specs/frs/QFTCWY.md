---
title: WebAssembly binding for the protocol core
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6VHDG2BG62PZDW7K4QFTCWY
created_at: 2026-11-07T17:39:28.900Z
changelog_category: Added
---

# WebAssembly binding for the protocol core

## Summary

A small binding crate exposes the protocol core to the web page.
It builds the requests the meter understands and decodes the replies into plain objects the page can use.
It also wraps the simulator, so the browser can talk to a demo meter that answers exactly as the desktop app's demo does.
Nothing in it performs input or output; the page moves the bytes.

## Requirement

Add a `powerz-wasm` crate (wasm-bindgen) with a sans-IO API over `powerz-protocol` and `powerz-sim`, and a build script that emits the wasm module and its JS glue for the web UI.

## Acceptance Criteria

- AC-QFTCWY.1: The crate exports request builders for CONNECT, auth (level, time and random supplied by the caller), GET_DATA with an attribute mask, queue start at a rate, queue stop, PD sniffer start/stop/poll and memory read, each returning the frame bytes the native session sends for the same inputs.
- AC-QFTCWY.2: It exports reply decoders for the connect/auth reply, ADC snapshot, ADC queue records (V, I, CC1, CC2, D+, D−, timestamps), PD poll (status samples and decoded PD messages in the shape the desktop PD table uses) and device info; malformed or short input returns a JS error with a kind, never a panic.
- AC-QFTCWY.3: It exports a demo meter that wraps the simulator's bulk persona on a caller-driven clock: `transact(bytes) → bytes` and `advance(ms)`; a session driven only through the exported builders, decoders and this demo meter authenticates, streams queue samples and captures PD messages.
- AC-QFTCWY.4: `bun run build:wasm` builds the crate in release mode for wasm32 and runs wasm-bindgen into a git-ignored folder in the web app; the gate chain runs it before the Vitest step, and the output module is at most 1.5 MB.
- AC-QFTCWY.5: Rust unit tests prove each builder's bytes equal the native session's request for the same inputs, and each decoder agrees with the native decoder on recorded vectors.

## Technical Design

New workspace crate `crates/powerz-wasm` (`crate-type = ["cdylib", "rlib"]`) depending on featureless `powerz-protocol`, `powerz-sim`, `wasm-bindgen` and `serde-wasm-bindgen`. Builders and decoders are thin wrappers over existing pure functions, so the parity tests run natively. A `DemoMeter` struct holds a `SimDevice` on the manual clock. Errors map to a JS object with `kind` and `message`. The script pins wasm-bindgen-cli to the crate's exact version and writes `apps/desktop/src/wasm/`, which is git-ignored.

## Testing

Native `cargo test -p powerz-wasm` covers parity against session requests and decoder vectors, and a driven sim session through the exported API. A Vitest smoke test loads the built module in Node and runs connect, auth and one queue poll against the demo meter.

## Notes

wasm-bindgen-cli must match the crate version exactly; CI installs it with a pinned version.
