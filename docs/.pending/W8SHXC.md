---
fr: W8SHXC
impact_set: {"symbols":[{"name":"MeterDriver","file":"apps/desktop/src/lib/web/driver.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"8e2053818d137fce766f1333c11f31aa330ac944263ebeb0e8efaba5f9061779"},{"name":"WEB_FEATURES","file":"apps/desktop/src/lib/web/features.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"4fbfeaabd9ca7db81a902c15dcc4460f48407f36aff34afeabfbd91c9d0549c3"},{"name":"portHeldError","file":"apps/desktop/src/lib/web/serial.ts","kind":"const","visibility":"public","change":"added","signatureHash":"b5cc4d170e5892ff32ed6ba7365804610cb40903b3dc74379d064e86e39cc937"},{"name":"WebSession","file":"apps/desktop/src/lib/web/session.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"c88305f7ef0e8c0ed92f8099dc272b102776a3c4fe895b343f02324f69530ab5"},{"name":"createWebBackend","file":"apps/desktop/src/lib/web/webBackend.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"9da133b4f785018fd588f255eaa7fd022bf8d1616d01ac528f91b7f99fbf02e3"},{"name":"WebBackendOptions","file":"apps/desktop/src/lib/web/webBackend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"4686db5210829fdedf301644264598ec9a686821abce94cdbcd5a086981a6566"},{"name":"default","file":"apps/desktop/vite.config.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"ada4dd119ce4306e29ee5818e9c909bf2e22dd4f80bed2cc0c8da49f5ec32780"}],"routes":[],"configKeys":[],"stateEvents":[{"kind":"action-type","name":"module","file":"apps/desktop/src/lib/web/webBackend.ts","change":"added"}]}
target_section: how-to
target_file: docs/how-to/web-edition.md
generated_at: 2026-11-09T15:43:37Z
---

## Trigger a charger in the browser

The Trigger tab works in Chrome and Edge through Web Serial on the meter's virtual
serial port (`WEB_FEATURES` switches triggering on). The browser runs the same trigger
engine as the desktop app (the `powerz-trigger` crate, compiled to WebAssembly), so the
safeguards are the same: a request above 5 V needs your confirmation, "don't ask again"
covers PD requests up to 20 V only and ends at 5 V, EPR requests and console lines always
ask, and leaving the panel, disconnecting or closing the session goes back to 5 V first.
The first session opens the browser's port picker (the same port the settings change
uses); a settings change waits while a trigger session holds the port.

Triggering needs the page to be cross-origin isolated: the web edition installs a small
service worker for that, so the first visit may ask for one reload. Closing the tab is
best effort: the page tries to return to 5 V, and asks before closing while a contract
above 5 V stands. A HID session can trigger too, since triggering uses only the serial port.
