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

const target = document.getElementById("app");
if (!target) throw new Error("missing #app mount point");

export default mount(App, { target });
