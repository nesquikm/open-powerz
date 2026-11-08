---
title: Protocol core builds for the browser
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6VHDG2BG62PZDW7K4QFTCWX
created_at: 2026-11-07T17:39:28.900Z
changelog_category: Added
---

# Protocol core builds for the browser

## Summary

The protocol library, the PD decoder and the simulator compile for WebAssembly.
The native USB, HID and serial code moves behind build switches.
The desktop app and the command-line tool keep those switches on.
The simulator can run on a clock the caller drives, because a browser cannot block on a system clock.
A new CI step builds all three for the browser, so the split cannot rot.

## Requirement

Make the pure core of the protocol crate (framing, decoders, auth/AES, memory and PD parsing) and the simulator build for wasm32-unknown-unknown, with the native transports behind cargo features, and prove the desktop and CLI builds are unchanged.

## Acceptance Criteria

- AC-QFTCWX.1: `powerz-protocol` has cargo features `bulk` (nusb), `hid` (hidapi) and `cdc` (serialport), all on by default; every module that touches a native transport, discovery or the PDM serial port is gated on its feature, and `cargo build -p powerz-protocol --no-default-features --target wasm32-unknown-unknown --locked` succeeds.
- AC-QFTCWX.2: `powerz-sim` builds for wasm32-unknown-unknown against the featureless protocol crate, and offers a constructor that runs on a caller-driven clock (milliseconds advanced by the caller), so no code path the browser uses calls `Instant::now` or `thread::sleep`.
- AC-QFTCWX.3: The auth random value on wasm32 comes from the browser's crypto source (getrandom's web backend); a unit test proves auth plaintext building still works with an injected random value.
- AC-QFTCWX.4: The CLI and desktop crates build with the same feature set as before; `cargo test --workspace --locked` passes with no test changed except new ones.
- AC-QFTCWX.5: CI's gate job adds the wasm32-unknown-unknown target in its wasm step only (as the PD decoder step does today; `rust-toolchain.toml` and every other job stay as they are) and builds `powerz-pd`, featureless `powerz-protocol` and `powerz-sim` for that target; the native `cargo build` / `cargo test --workspace --locked` commands and the desktop bundle are unchanged.

## Technical Design

Add `[features]` with `default = ["bulk", "hid", "cdc"]`; make nusb and serialport optional. Gate `transport::{bulk, cdc, discovery}`, `firmware::flash` and `pdm::port` with `#[cfg(feature = ...)]`; the pure CDC framing helpers move to an ungated `transport::cdc_frame`. The workspace dependency sets `default-features = false`; the CLI and desktop list `bulk`, `hid`, `cdc` explicitly, and the sim enables them on non-wasm targets only. `getrandom` gets a wasm32 target dependency with `wasm_js` (enough in 0.4, no `--cfg`). `SimDevice::manual` starts a sim clock at 0 that `advance(ms)` moves. Protocol `SystemClock` panics on wasm32, so the binding supplies its own clock.

## Testing

Existing workspace tests prove behaviour is unchanged. New unit tests cover the sim's manual clock (advance moves queue timestamps) and the random-injection path. The wasm builds run as CI steps and in the local gate.

## Notes

The wasm build of the protocol crate is a compile check, not a runtime test; runtime coverage comes from the binding crate and the browser tests.
