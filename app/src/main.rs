// Windows: no console window for a GUI build (release only, so `cargo run`
// still shows panics and the server's stderr during development).
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

use treesight_shell::ShellExt;

/// What this binary is, filled in by `build.rs`. Handed to the shell as
/// `ShellExt::build` so the footer and `--version` name the program rather than
/// the library it is built on — the same way every downstream app does it.
const BUILD: treestamp::BuildInfo = treestamp::build_info!("TREESIGHT_");

fn main() {
    // Answered here rather than in the shell, which is also the mobile entry
    // point and has no command line to read. On a release Windows build this
    // prints into a subsystem with no console attached — the trade every GUI
    // binary makes, and the page footer is the answer there.
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("{}", BUILD.report());
        return;
    }
    // `generate_context!` is why this crate exists apart from the shell: it
    // compiles *this* directory's tauri.conf.json, icons and capabilities in.
    treesight_shell::run_with(
        tauri::generate_context!(),
        ShellExt { build: Some(BUILD), ..Default::default() },
    );
}
