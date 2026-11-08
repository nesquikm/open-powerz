---
title: Static site and GitHub Pages deploy
milestone: M_QFTCWW
status: active
archived_at: null
id: fr_01M6VHDG2BG62PZDW7K4QFTCX1
created_at: 2026-11-07T17:39:28.900Z
changelog_category: Added
---

# Static site and GitHub Pages deploy

## Summary

The web edition builds into a static site served from the project's GitHub Pages address.
It carries the project mascot as its icon.
Browsers without USB access get a clear page that says what is supported.
A release now also deploys the site, and a maintainer can deploy the main branch by hand.
Browser end-to-end tests run against the built site with the demo meter.

## Requirement

Add a static Pages build with base path `/open-powerz/`, the brand favicon, a "not supported" page, Playwright tests of the built site with the demo meter, and a Pages deploy job in the release workflow plus a manual dispatch for main.

## Acceptance Criteria

- AC-QFTCX1.1: `bun run build:pages` produces a static folder with a working page under base path `/open-powerz/`: every asset URL resolves when the folder is served at that path, and the wasm module loads.
- AC-QFTCX1.2: The page uses the brand favicon (SVG plus 16/32 PNG) and a title and description for the web edition.
- AC-QFTCX1.3: In a browser without `navigator.usb` and `navigator.hid` the page shows a "not supported" view naming Chrome and Edge, linking the desktop downloads, and still offering the demo meter.
- AC-QFTCX1.4: A Playwright suite serves the built site at `/open-powerz/` in Chromium, opens the demo meter, sees live readings and the chart, starts and stops a recording that downloads a CSV, runs a PD capture, and passes an axe scan; a WebKit run sees the not-supported view.
- AC-QFTCX1.5: `release.yml` gains a `pages` job that runs after the check job on a `v*` tag, builds the site and deploys it with `actions/upload-pages-artifact` and `actions/deploy-pages` (permissions `pages: write`, `id-token: write`, environment `github-pages`); it is skipped on pull-request dry runs.
- AC-QFTCX1.6: A `pages.yml` workflow with `workflow_dispatch` deploys main the same way; both workflows share one concurrency group for Pages.

## Technical Design

A Vite mode `pages` sets `base: "/open-powerz/"`, `VITE_BACKEND=web` and an output folder `dist-pages`; brand icons are copied from the brand folder at build time, not duplicated. Support detection runs in `App.svelte` as it mounts, before any backend call. The deploy steps live in a reusable workflow called from both workflows.

## Testing

Playwright with one worker over a static server that mounts the build under the base path. Workflow files are linted with actionlint where present and covered by the existing release-workflow tests.

## Notes

Pages must be enabled in the repository settings with the source set to GitHub Actions; the FR does not change repository settings.
