//! Startup frame fit (FR EWQ6FQ, AC .1): a window restored off every connected
//! screen moves back to the centre of the main screen. Pure geometry, all values in
//! logical px with y pointing down; [`fit_main_window`] wires it to Tauri.

/// The height of a window's title strip, the part the user drags it by.
const TITLE_STRIP_HEIGHT: f64 = 40.0;
/// How much of the title strip one screen must hold for the window to stay put.
const MIN_VISIBLE_WIDTH: f64 = 100.0;

/// A window frame or a screen area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A connected screen: its whole frame and its work area (the frame minus menu
/// bar, dock or task bar).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub frame: Rect,
    pub work_area: Rect,
}

/// The overlap of `a` and `b` as (width, height), zero when they do not meet.
fn overlap(a: Rect, b: Rect) -> (f64, f64) {
    let w = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let h = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    (w.max(0.0), h.max(0.0))
}

/// Where the window should go, or `None` when it stays where it is: it is visible
/// enough (one screen holds a 100 × 40 px piece of its title strip) or no screen
/// was reported. Otherwise its size is kept, shrunk to the main screen's work
/// area, and it is centred in that work area.
pub fn fit_frame(frame: Rect, screens: &[Screen], main: Option<&Screen>) -> Option<Rect> {
    let main = main?;
    if screens.is_empty() {
        return None;
    }
    let strip = Rect {
        height: TITLE_STRIP_HEIGHT,
        ..frame
    };
    let visible = screens.iter().any(|s| {
        let (w, h) = overlap(strip, s.frame);
        w >= MIN_VISIBLE_WIDTH && h >= TITLE_STRIP_HEIGHT
    });
    if visible {
        return None;
    }
    let area = main.work_area;
    let width = frame.width.min(area.width);
    let height = frame.height.min(area.height);
    Some(Rect {
        x: area.x + (area.width - width) / 2.0,
        y: area.y + (area.height - height) / 2.0,
        width,
        height,
    })
}

/// A physical position and size, divided by `scale`, as a logical [`Rect`].
fn logical_rect(
    pos: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    scale: f64,
) -> Rect {
    Rect {
        x: f64::from(pos.x) / scale,
        y: f64::from(pos.y) / scale,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
    }
}

fn monitor_screen(m: &tauri::Monitor) -> Screen {
    let scale = m.scale_factor();
    let work = m.work_area();
    Screen {
        frame: logical_rect(*m.position(), *m.size(), scale),
        work_area: logical_rect(work.position, work.size, scale),
    }
}

/// Moves `window` back onto the main screen when its frame lies off every
/// connected screen. Any failure to read the window or the monitors leaves the
/// window where it is.
pub fn fit_main_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    let read = || -> tauri::Result<Option<(Rect, Rect, bool)>> {
        let scale = window.scale_factor()?;
        let pos = window.outer_position()?;
        let outer = window.outer_size()?;
        let inner = window.inner_size()?;
        let frame = logical_rect(pos, outer, scale);
        let screens: Vec<Screen> = window
            .available_monitors()?
            .iter()
            .map(monitor_screen)
            .collect();
        let main = window.primary_monitor()?.map(|m| monitor_screen(&m));
        // The decorations around the content, so the outer frame maps back to the
        // inner size `set_size` takes.
        let deco = Rect {
            x: 0.0,
            y: 0.0,
            width: f64::from(outer.width.saturating_sub(inner.width)) / scale,
            height: f64::from(outer.height.saturating_sub(inner.height)) / scale,
        };
        Ok(fit_frame(frame, &screens, main.as_ref()).map(|to| {
            let resize = to.width < frame.width || to.height < frame.height;
            (to, deco, resize)
        }))
    };
    let Ok(Some((to, deco, resize))) = read() else {
        return;
    };
    if resize
        && let Err(e) = window.set_size(tauri::LogicalSize::new(
            (to.width - deco.width).max(1.0),
            (to.height - deco.height).max(1.0),
        ))
    {
        eprintln!("open-powerz: could not resize the window onto the screen: {e}");
    }
    if let Err(e) = window.set_position(tauri::LogicalPosition::new(to.x, to.y)) {
        eprintln!("open-powerz: could not move the window onto the screen: {e}");
    }
}
