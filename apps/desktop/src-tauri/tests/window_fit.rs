//! The window that went sightseeing (FR EWQ6FQ, AC .1): at startup a window whose
//! restored frame lands off every connected screen moves back to the main screen.
//!
//! API pinned here, beyond the FR's Technical Design (all values logical px, y down):
//!
//! - `window_fit::Rect { x, y, width, height }` (`f64` fields; `Copy`, `PartialEq`,
//!   `Debug`): a window frame or a screen area;
//! - `window_fit::Screen { frame, work_area }`: a connected screen's whole frame and
//!   its work area (the frame minus menu bar, dock or task bar);
//! - `window_fit::fit_frame(frame, screens, main) -> Option<Rect>` with
//!   `frame: Rect`, `screens: &[Screen]`, `main: Option<&Screen>`:
//!   - the window's title strip is the top 40 px of its frame; it is visible
//!     enough when the strip overlaps one screen's frame by at least 100 × 40 px;
//!   - visible enough, or no screen reported (`screens` empty or `main` `None`):
//!     `None`, the window stays where it is;
//!   - otherwise `Some(new_frame)`: the size kept but shrunk to fit the main
//!     screen's work area, centred in that work area.

use open_powerz_desktop_lib::window_fit::{Rect, Screen, fit_frame};

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// A 1512 × 982 MacBook screen at the origin, its work area under a 33 px menu bar.
fn laptop() -> Screen {
    Screen {
        frame: rect(0.0, 0.0, 1512.0, 982.0),
        work_area: rect(0.0, 33.0, 1512.0, 949.0),
    }
}

/// A 1920 × 1080 external display to the right of the laptop, no menu bar.
fn external() -> Screen {
    Screen {
        frame: rect(1512.0, 0.0, 1920.0, 1080.0),
        work_area: rect(1512.0, 0.0, 1920.0, 1080.0),
    }
}

/// The 1280 × 800 window of the r3 tester run.
const W: f64 = 1280.0;
const H: f64 = 800.0;

#[test]
fn off_every_screen_moves_to_the_centre_of_the_main_work_area() {
    // r3: the window came back at y = −987, far above every screen.
    let main = laptop();
    let screens = [main];
    let got = fit_frame(rect(100.0, -987.0, W, H), &screens, Some(&main));
    // Centred in the work area (0, 33, 1512 × 949), size kept.
    assert_eq!(got, Some(rect(116.0, 33.0 + 74.5, W, H)));
}

#[test]
fn off_every_screen_after_the_second_display_is_removed() {
    // The window was on the external display, which is gone now.
    let main = laptop();
    let screens = [main];
    let got = fit_frame(rect(2000.0, 100.0, W, H), &screens, Some(&main));
    assert_eq!(got, Some(rect(116.0, 107.5, W, H)));
}

#[test]
fn a_window_on_the_second_screen_stays() {
    let main = laptop();
    let screens = [main, external()];
    assert_eq!(
        fit_frame(rect(2000.0, 100.0, W, H), &screens, Some(&main)),
        None
    );
}

#[test]
fn a_window_fully_on_screen_stays() {
    let main = laptop();
    let screens = [main];
    assert_eq!(
        fit_frame(rect(50.0, 60.0, W, H), &screens, Some(&main)),
        None
    );
}

#[test]
fn a_maximised_window_at_the_very_top_of_the_screen_stays() {
    // The title strip under the menu bar area still lies on the screen's frame.
    let main = laptop();
    let screens = [main];
    assert_eq!(
        fit_frame(rect(0.0, 0.0, 1512.0, 982.0), &screens, Some(&main)),
        None
    );
}

#[test]
fn partly_visible_with_enough_title_strip_stays() {
    // Mostly past the right edge, but 150 px of the title strip is on the screen.
    let main = laptop();
    let screens = [main];
    assert_eq!(
        fit_frame(rect(1512.0 - 150.0, 200.0, W, H), &screens, Some(&main)),
        None
    );
    // Exactly 100 × 40 px of the strip is enough.
    assert_eq!(
        fit_frame(rect(1512.0 - 100.0, 200.0, W, H), &screens, Some(&main)),
        None
    );
}

#[test]
fn partly_visible_with_too_little_title_strip_moves() {
    let main = laptop();
    let screens = [main];
    // Only 99 px of the strip's width is on the screen.
    assert_eq!(
        fit_frame(rect(1512.0 - 99.0, 200.0, W, H), &screens, Some(&main)),
        Some(rect(116.0, 107.5, W, H))
    );
    // The strip pokes 20 px into the screen from above: the body is visible, the
    // title strip (and so the way to drag the window) is not.
    assert_eq!(
        fit_frame(rect(100.0, -20.0, W, H), &screens, Some(&main)),
        Some(rect(116.0, 107.5, W, H))
    );
    // The strip is below the bottom edge, only 30 px of it on the screen.
    assert_eq!(
        fit_frame(rect(100.0, 982.0 - 30.0, W, H), &screens, Some(&main)),
        Some(rect(116.0, 107.5, W, H))
    );
}

#[test]
fn a_strip_split_between_two_screens_counts_per_screen() {
    // 60 px of the strip on each screen: no single screen holds 100 px of it.
    // The external display sits right of the laptop, top-aligned.
    let main = laptop();
    let screens = [main, external()];
    // Narrow window straddling the seam: 60 px left of it, 60 px right of it.
    let got = fit_frame(
        rect(1512.0 - 60.0, 100.0, 120.0, 300.0),
        &screens,
        Some(&main),
    );
    assert_eq!(got, Some(rect(696.0, 33.0 + 324.5, 120.0, 300.0)));
}

#[test]
fn larger_than_the_screen_and_off_it_shrinks_to_the_work_area() {
    let main = laptop();
    let screens = [main];
    let got = fit_frame(
        rect(-5000.0, -3000.0, 2560.0, 1440.0),
        &screens,
        Some(&main),
    );
    assert_eq!(got, Some(rect(0.0, 33.0, 1512.0, 949.0)));
}

#[test]
fn wider_only_shrinks_only_the_width() {
    let main = laptop();
    let screens = [main];
    let got = fit_frame(rect(4000.0, 4000.0, 2000.0, 600.0), &screens, Some(&main));
    assert_eq!(got, Some(rect(0.0, 33.0 + 174.5, 1512.0, 600.0)));
}

#[test]
fn larger_than_the_screen_but_visible_stays_as_it_is() {
    // A window bigger than the screen whose title strip is on it is left alone.
    let main = laptop();
    let screens = [main];
    assert_eq!(
        fit_frame(rect(-100.0, 40.0, 2560.0, 1440.0), &screens, Some(&main)),
        None
    );
}

#[test]
fn the_main_screen_need_not_be_the_first_or_at_the_origin() {
    // The main screen is the external one; the laptop is listed first.
    let main = external();
    let screens = [laptop(), main];
    let got = fit_frame(rect(0.0, -2000.0, W, H), &screens, Some(&main));
    assert_eq!(got, Some(rect(1512.0 + 320.0, 140.0, W, H)));
}

#[test]
fn no_screen_reported_leaves_the_window_alone() {
    assert_eq!(fit_frame(rect(100.0, -987.0, W, H), &[], None), None);
    // Screens but no main screen: nowhere to move it to.
    let screens = [laptop()];
    assert_eq!(fit_frame(rect(100.0, -987.0, W, H), &screens, None), None);
}
