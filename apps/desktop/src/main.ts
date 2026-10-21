// First: a C/POSIX locale must be fixed before uPlot reads navigator.language.
import "./lib/localeGuard";
import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
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

export default mount(App, { target });
