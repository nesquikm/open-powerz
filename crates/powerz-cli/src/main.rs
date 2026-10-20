fn main() {
    println!(
        "powerz {} (open-powerz, unofficial). Looking for VID 0x{:04X}; device commands are not implemented yet.",
        env!("CARGO_PKG_VERSION"),
        powerz_protocol::VID
    );
}
