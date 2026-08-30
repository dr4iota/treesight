fn main() {
    // The empty capabilities/ next door is load-bearing. tauri_build tells cargo
    // to watch that directory, and cargo treats a watched path that does not
    // exist as permanently dirty — so without it this build script re-ran on
    // every build and rebuilt everything downstream of it. This app defines no
    // capabilities (it talks HTTP, not IPC), hence empty rather than absent.
    tauri_build::build();
    // Which commit this binary is, for its footer and its `--version`. Read back
    // by `treestamp::build_info!` in `main` under the same prefix, and handed to
    // the shell as `ShellExt::build` — the shell's own stamp names the library.
    treestamp::Stamp::new(env!("CARGO_MANIFEST_DIR"), "TREESIGHT_").emit();
}
