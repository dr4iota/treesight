fn main() {
    // What this *library* is, which is the fallback `ShellExt::build` uses when a
    // program does not name itself. Under its own prefix, so it cannot be confused
    // with the stamp `app` emits for the binary: there is no tauri.conf.json here
    // to take a productName from, and the honest answer is the crate's own name.
    treestamp::Stamp::new(env!("CARGO_MANIFEST_DIR"), "TREESIGHT_SHELL_").emit();

    // `desktop` and `mobile`, which `tauri_build::build()` would set if this crate
    // called it. It must not — see the note in Cargo.toml — so they are set here,
    // the same six lines every Tauri plugin crate carries for the same reason.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let mobile = target_os == "ios" || target_os == "android";
    alias("desktop", !mobile);
    alias("mobile", mobile);
}

fn alias(name: &str, set: bool) {
    println!("cargo:rustc-check-cfg=cfg({name})");
    if set {
        println!("cargo:rustc-cfg={name}");
    }
}
