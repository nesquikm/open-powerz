# open-powerz brand art

The electron-surf mascot: a small electron riding a blue power wave.

## Licence

This is original art for this project under the MIT licence, the same licence as the rest of the repository.
The lettering is drawn from stroke glyphs, with no font in any file.
The art carries no ChargerLAB or POWER-Z marks.

## Assets

Every SVG is the source and the PNG next to it is its render.

| Source                    | Renders                             | Use                                                       |
| ------------------------- | ----------------------------------- | --------------------------------------------------------- |
| `meter-logo.svg`          | `meter-logo.png` (200×200)          | Startup logo for the meter, alpha 0 or 255 only           |
| `meter-logo-wordmark.svg` | `meter-logo-wordmark.png` (200×200) | Meter startup logo with the wordmark, alpha 0 or 255 only |
| `app-icon.svg`            | `app-icon-1024.png` (1024×1024)     | Desktop app icon                                          |
| `readme-banner.svg`       | `readme-banner.png` (1280×400)      | Banner at the top of the repository README                |
| `social-preview.svg`      | `social-preview.png` (1280×640)     | GitHub social preview                                     |
| `favicon.svg`             | `favicon-16.png`, `favicon-32.png`  | Small-size icon                                           |

The two meter logos keep a transparent margin of at least 3 px on every side.
The art ends inside the 200×200 frame, so the meter's own background shows around it.
The meter centres the logo on its larger screen, so a hard edge at the frame would show as a box.
Every edge where art meets transparency is dark outline ink.

`meter-logo.svg` is a hand-kept source. The generator writes the other SVGs and every PNG.

The desktop app uses `app-icon.svg` as its icon source and `favicon.svg` as its small-size source.

The social preview is set by hand in the GitHub repository settings (Settings → General → Social preview).

The favicon is kept for the web edition and is not wired into any page yet.

## Re-render

You need Python 3 with Pillow, `rsvg-convert` (librsvg), and a debug build of the `powerz` CLI for the meter-logo preview checks:

```bash
cargo build -p powerz-cli
python3 -I assets/brand/tools/render.py <out-dir>
```

Run it from the repository root.
`tools/render.py` uses `tools/kit_svgs.py` to write the generated SVGs, then re-renders every PNG in this folder.
It also writes an on-meter preview of both meter logos, `<name>-onmeter.png`, to `<out-dir>`.
On-meter mocks and preview checks go to `<out-dir>`, never into the repository.
