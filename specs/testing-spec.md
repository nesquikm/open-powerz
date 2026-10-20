# Testing Specification

## 1. Test Framework
- **Runner:** Rust built-in harness via `cargo test --workspace --locked`; Vitest 5 (`bun run test`) for TypeScript
- **Mocking:** hand-written fake transports implementing the protocol crate's transport trait; no mocking crate. Vitest `vi.fn()` / `vi.mock()` for the Tauri `invoke` boundary in UI logic tests
- **Coverage:** `cargo llvm-cov` for Rust (when coverage is wired into CI); Vitest v8 coverage for TS

## 2. Test Structure

**Layout policy: `src/`-co-located** — tests live next to the source they exercise (toolkit default).

- **Rust:** unit tests in a `#[cfg(test)] mod tests` at the bottom of each module; cross-module
  tests in `crates/<crate>/tests/*.rs`; byte-vector fixtures in `crates/powerz-protocol/tests/vectors/`.
- **TypeScript:** each `apps/desktop/src/lib/<module>.ts` has a sibling `<module>.test.ts`.

```
crates/powerz-protocol/src/
└── lib.rs              # + #[cfg(test)] mod tests (one per module as modules land)
apps/desktop/src/lib/
├── units.ts
└── units.test.ts
```

Planned: a `tests` directory inside the protocol crate holding integration tests and a
`vectors` folder of our own hex fixture files rewritten from device logs.

## 3. Conventions

### Naming
- Rust: test fns named for the behavior (`parses_adc_section_of_44_bytes`); files `*.rs` under `tests/`
- TS: `*.test.ts`; `describe` per function, `it` states the expected behavior

### What to Test
- Every decoder and frame builder against byte vectors from the vendor docs' worked examples and our own device captures (tag each vector with its source: `[doc]`, `[device]`)
- Malformed input: truncated sections, unknown attributes, wrong lengths (must return errors, never panic)
- Session logic (one request in flight, timeouts, reconnect) with fake transports
- Unit conversion and formatting in the UI layer

### What NOT to Test
- Tauri/WebView internals, third-party crates, generated files under `src-tauri/gen/`
- Real hardware in CI. Device tests are manual, run through the CLI, and their logs become fixtures

### End-to-end and hardware-in-the-loop
- **Simulator e2e (in the gate):** the `powerz` binary is run with `assert_cmd` against the
  `powerz-sim` demo device on Linux, macOS and Windows; human-readable output is
  snapshot-tested with `insta`.
- **HIL (opt-in):** the same flows against a real meter run only with `POWERZ_HIL=1`, using
  only the safe command set in `docs/protocol.md`; CI never sets it.
- **Future (desktop, M_R4WG4Q):** Playwright e2e against the Tauri app wired to the demo
  device (connect, live values, chart, record, export), plus visual-check screenshots.
- **Future (Linux CI):** expose the simulator as a real USB device (dummy_hcd + FunctionFS
  gadget, or USB/IP) so the nusb bulk path is tested end to end.

## 4. Coverage Targets
| Layer   | Target | Minimum |
|---------|--------|---------|
| `powerz-protocol` decode/framing | >=90%  | 80%     |
| Overall | >=75%  | 65%     |

## 5. Test Data
Byte-vector fixtures written in our own files (hex text plus a comment naming the source and
firmware version). Serial numbers and UIDs in device captures are redacted before they are committed.
