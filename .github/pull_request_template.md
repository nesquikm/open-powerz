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
- [ ] Memory soak (macOS app): stream at 50 SPS for 1 hour with the window visible; note the app's `com.apple.WebKit.WebContent` RSS (`ps -o rss= -p <pid>`) after 2 min of warm-up and at the end (should grow by less than 50 MB)
