const ICON: &str = "../../assets/icons/ado-planner.ico";

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    // build.rs runs on the host, so check the target instead of using #[cfg].
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // GPUI loads icon resource ID 1 for the window class, and Explorer, the
    // taskbar, Start menu shortcuts, and Apps & features read the same resource.
    winresource::WindowsResource::new()
        .set_icon(ICON)
        .set("ProductName", "ADO Planner")
        .set("FileDescription", "ADO Planner")
        .compile()
        .expect("embed the Windows icon resource");
}
