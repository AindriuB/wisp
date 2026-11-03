// Embeds the Wisp icon in the Windows exe. Resource ID 1 is also what GPUI's
// window class loads (`load_icon`), so it sets exe, titlebar and taskbar icons.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icon/wisp.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon_with_id("../../assets/icon/wisp.ico", "1");
        res.compile().expect("embed Windows icon resource");
    }
}
