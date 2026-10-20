/// Returns the app and protocol-library version for the About screen.
#[tauri::command]
fn app_info() -> String {
    format!(
        "open-powerz {} (VID 0x{:04X})",
        env!("CARGO_PKG_VERSION"),
        powerz_protocol::VID
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("error while running the open-powerz desktop app");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_names_the_vendor_id() {
        assert!(app_info().contains("0x5FC9"));
    }
}
