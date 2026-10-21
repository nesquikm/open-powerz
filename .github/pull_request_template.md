## Summary

<!-- What changed and why. Add `Refs: <FR>` lines for the FRs this covers. -->

## Checklist

- [ ] The gate passes locally (see "Gating rule" in `CLAUDE.md`)
- [ ] UI changes have visual-check screenshots (light and dark, ~900 and ~1600 px)

## Manual hardware checks (when a meter is available)

- [ ] `POWERZ_HIL=1 cargo test -p open-powerz-desktop --locked --test hil` passes against a real meter
- [ ] Unplug the meter mid-stream: the app shows `disconnected` within 2 s and the samples are kept
- [ ] Replug the meter: it is listed again within 2 s, and reconnecting appends to the session with a gap event
- [ ] macOS real-device app run: visual-check screenshots in light and dark, at ~900 and ~1600 px wide
