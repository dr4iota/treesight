// What this binary says it is: the version, the commit it was cut from, how
// many tracked files differed from that commit, and when it was compiled.
// `treestamp::build_info!("TREESERVE_")` in `src/main.rs` reads them back.
//
// `.repo` because this crate *is* the repository root. `Stamp::new` otherwise
// asks git about the manifest directory's **parent**, which is right for
// `app/` and wrong here — in a checkout vendored as a submodule that parent is
// the repository that vendored us, and the stamp would name somebody else's
// commit.
fn main() {
    treestamp::Stamp::new(env!("CARGO_MANIFEST_DIR"), "TREESERVE_")
        .repo(env!("CARGO_MANIFEST_DIR"))
        .emit();
}
