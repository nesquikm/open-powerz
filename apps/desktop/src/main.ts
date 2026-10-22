// First: a C/POSIX locale must be fixed before uPlot reads navigator.language.
import "./lib/localeGuard";
import { mount, tick, unmount } from "svelte";
import App from "./App.svelte";
import { attach } from "./lib/backend";
import "./app.css";
import { RELOAD_HOOK, type ReloadHook } from "../e2e/hook";
import {
  applyTheme,
  browserStorage,
  loadTheme,
  resolveTheme,
  systemPrefersDark,
} from "./lib/theme";

// Paint the persisted theme before the first render to avoid a flash.
applyTheme(
  resolveTheme(loadTheme(browserStorage()), systemPrefersDark(window.matchMedia?.bind(window))),
  document.documentElement,
);
// Resolve styles once before mounting. Without it, WebKit (Safari, WKWebView)
// can give nodes mounted in the same task the pre-theme inherited style (no
// palette variables), e.g. a black rate picker on the dark top bar.
void getComputedStyle(document.body).color;

const target = document.getElementById("app");
if (!target) throw new Error("missing #app mount point");

let app = mount(App, { target });

// The mock build (the Playwright UI suite) simulates a window reload: a fresh
// app on the same page while the in-page mock backend keeps its session, as
// the real backend outlives a webview reload (FR VEDDFD).
if (import.meta.env.VITE_BACKEND === "mock") {
  const reload: ReloadHook = async () => {
    // The old page stops receiving first, as its channel does on a real reload.
    await attach(() => {});
    await unmount(app);
    app = mount(App, { target });
    await tick();
  };
  (window as unknown as Record<string, ReloadHook>)[RELOAD_HOOK] = reload;
}

export default app;
