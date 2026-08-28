// Windows: no console window for a GUI build (release only, so `cargo run`
// still shows panics and the server's stderr during development).
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

fn main() {
    // Answered here rather than in `run`, which is also the mobile entry point
    // and has no command line to read. On a release Windows build this prints
    // into a subsystem with no console attached — the trade every GUI binary
    // makes, and the page footer is the answer there.
    if std::env::args().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("{}", treesight::BUILD.report());
        return;
    }
    treesight::run()
}
