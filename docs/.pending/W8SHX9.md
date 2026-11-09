---
fr: W8SHX9
impact_set: {"symbols":[{"name":"DemoTransport","file":"apps/desktop/src/lib/web/demoTransport.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"11e0cb1d61f24c24cb02b8f774bb1a75718e5094374d67b8c011003284732129"},{"name":"MeterDriver","file":"apps/desktop/src/lib/web/driver.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"b5ef087430447932ff13fc21f5d814677a03c1a475789a8f4581255e16bc7eb4"},{"name":"PowerzWasm","file":"apps/desktop/src/lib/web/driver.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"790ea9fcf98d9c1e32d57fe0fd535a9c30a00edd0d0b011932ee94ccabea6cb1"},{"name":"SettingsWriteLike","file":"apps/desktop/src/lib/web/driver.ts","kind":"interface","visibility":"public","change":"added","signatureHash":"05c2e7cfb77cbe6e78d30127a3636b19aa01c489c09ae5eb0d62201d1af7dcde"},{"name":"WEB_FEATURES","file":"apps/desktop/src/lib/web/features.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"c9a8ec5015992357b7324c20c807eb4b71bd011a4086ad59600d899a51024cb6"},{"name":"WebSession","file":"apps/desktop/src/lib/web/session.ts","kind":"class","visibility":"public","change":"modified","signatureHash":"9a60a4e0ec88aa69195d16abd723555983e62332ff039e0599e68c9db0230678"},{"name":"SESSION_COMMANDS","file":"apps/desktop/src/lib/web/session.ts","kind":"const","visibility":"public","change":"modified","signatureHash":"aaa06c2e08e13f36d81de796f3f4cd1c7abc2071a2967ce44925d5dcd2fbadc5"},{"name":"WebSessionOptions","file":"apps/desktop/src/lib/web/session.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"3d306bbabbeb746fe2ced0c96891781d256fd4451bd8009f98540ac12326b4e6"},{"name":"notSafeError","file":"apps/desktop/src/lib/web/transport.ts","kind":"const","visibility":"public","change":"added","signatureHash":"558982257174136f4563b53f3a9f24bb08276c99efeee658346cc6532e93142e"},{"name":"createWebBackend","file":"apps/desktop/src/lib/web/webBackend.ts","kind":"function","visibility":"public","change":"modified","signatureHash":"ceaad97d283f27c0b7ea7c0ee170cad3325d9ea0d767be6874aa37119972eeea"},{"name":"WebBackendOptions","file":"apps/desktop/src/lib/web/webBackend.ts","kind":"interface","visibility":"public","change":"modified","signatureHash":"0c2238aef93ac8ff51ec351835da24235dad05b9149a9fa34c9886e95ff91487"}],"routes":[],"configKeys":[],"stateEvents":[]}
target_section: how-to
target_file: docs/how-to/web-edition.md
generated_at: 2026-11-09T12:18:35Z
---

## Change meter settings in the browser

The meter takes a settings change only on its virtual serial port, so the web edition
sends it there through Web Serial (Chrome and Edge). `WEB_FEATURES` now switches the
settings write on: in the Meter settings panel, Edit and Apply work as in the desktop app.
The first change opens the browser's port picker, filtered to POWER-Z ports; pick the
meter's port and the browser remembers it, so later changes do not ask again.

The safeguards are the desktop app's: two SETTINGS reads that must match, one settings
write built by the protocol core, and a read-back that must show the change. Calibration
is never written, and nothing is retried. If another program holds the port, close it and
apply again. A HID session cannot change settings.
