//! AC-AXHK2K.5: an opened PD file goes only when the page says so. The
//! behaviour with a running manager is in the unit tests of `src/lib.rs`
//! (`AppState` is private); this pins the wiring those tests cannot see.

/// `src/lib.rs` without its test module.
fn app_source() -> &'static str {
    let source = include_str!("../src/lib.rs");
    source
        .split_once("#[cfg(test)]\nmod tests")
        .map_or(source, |(app, _)| app)
}

#[test]
fn the_page_has_a_command_that_releases_the_opened_capture() {
    let source = app_source();
    let handlers = source
        .split_once("generate_handler![")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once(']'))
        .map(|(names, _)| names)
        .expect("the command list");
    assert!(
        handlers
            .split(',')
            .any(|name| name.trim() == "pd_release_opened"),
        "pd_release_opened is not a command of the app"
    );
}

#[test]
fn no_connect_path_releases_the_opened_capture() {
    assert!(
        !app_source().contains("set_on_connected"),
        "a connected session, automatic reconnects included, still releases the opened capture"
    );
}
