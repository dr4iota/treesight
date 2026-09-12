# Further work

Deferred work on this repo, each item with the **trigger** that unblocks it.
Nothing here is started early: every one of them is a seam whose shape is
supposed to be decided by a downstream app that has already hit the missing
edge, and guessing at that shape first is how a public API acquires something
nobody wanted.

"Downstream" throughout means the private companion repo (working name
**telesight**) that vendors this one and fills `Vfs` + `ShellExt`; see
[architecture.md](architecture.md), *What this repo is not*.

## House rules

- One item, one commit. Subjects are a single plain sentence saying what the
  change does for the reader of the code (`git log --oneline -15` for the
  register).
- `scripts/snap.sh` before and after, always: `scripts/snap.sh /tmp/snapwork
  base`, the change, `… new`, `diff -r`. Each item below says which outcome is
  correct — **IDENTICAL** (any diff is a bug) or **REVIEWED DIFF** (exactly the
  described change and nothing else).
- `cargo test`, `cargo test -p treesight-shell`, and
  `cargo check --no-default-features --features pure` green; no new warnings.
- Served pages stay zero-JavaScript.
- `root_id_is_local` is the only RootId grammar. Never re-derive it.
- Stop and ask rather than improvise when a change seems to need JS on a served
  page, when a snapshot diff appears that you cannot fully explain, or when an
  item's description here disagrees with what the code actually looks like.

---

## Open from the last round

**Tested by hand and working on the desktop:** Download (the Save As copy, which
this section had been asking about since before it was rewritten around the
navigation callback), the Up button, and the one-screen layout in a browser.

**Still unexercised anywhere.** Everything Android, and one thing that is not:
an overlay scrollbar — GTK, or macOS unless the reader has asked for bars that
stay — passes over the last few pixels of the pane while it is in use, and
`--bar-strip` in `app.css` is six pixels of padding betting that it clears the
marks at the ends of the rows. One number, one place, if it does not.

---

## 1. `RootOpener` — *done*

Landed as `ShellExt::openers`, designed against what downstream actually needed
rather than the sketch this item carried. The differences are the interesting
part: the opener owns failure outright and answers `Option<Opened>`, because a
dismissed password box is a failure with nothing to say and a
`Result<_, String>` has no way to spell that. `Opened` carries the name the
window's title wants. `label` names the root on the wait page *before* it opens.
And `probe` is the non-connecting status this crate used to skip remote ids
for — a session the death hook buried, or a grant revoked in Settings, greys its
row at launch now instead of looking healthy until it is clicked.

Downstream's two copies of the choreography are gone; what is left there is the
dial and the grant lookup.

## 2. Embedder controls in the header — *done, and not as `show_term`*

Landed as `treeserve::HeaderFlag` plus `ShellExt::flags` and
`Config::set_flags`, rather than the `show_term: bool` this item asked for. A
boolean would have made this crate learn the word "terminal"; a list of
(href, icon, label, title, roots) makes it learn nothing, and the next thing a
downstream shell mounts — WebDAV, FTP, whatever — brings its own button on the
same seam. `roots` is a RootId prefix, which is how a control that acts on the
root stays off the roots it cannot act on, without this side knowing what any
of them are.

## 3. Mobile-capable `run_with` — *gating done, the rest open*

Done: single-instance, `desktop_dir` and the folder picker are `#[cfg(desktop)]`;
mobile roots at `app_data_dir()` (created on first use, empty to begin with) and
Places lists **Files** first. `cargo check --target aarch64-linux-android`
passes for `treesight` and for the downstream shell. Snapshot: **IDENTICAL**.

Still open:

- `aarch64-apple-ios` is unconfirmed — it needs macOS and Xcode.
- No folder picker on mobile. `ask_for_folder` says so rather than doing it.
  Choosing a directory means a per-directory grant: `ACTION_OPEN_DOCUMENT_TREE`
  plus `takePersistableUriPermission` on Android, `UIDocumentPickerViewController`
  plus a security-scoped bookmark on iOS. Both hand back a `content://` URI or a
  scoped URL, **not a path**, so the backend is a new `Vfs` — which is the seam
  that already exists — and not a tweak to `LocalFs`. Android needs a small
  Kotlin Tauri plugin; there is no first-class one.
- An offline cache the user *relies on* belongs in `app_data_dir()`: the cache
  directory is purgeable on both platforms, and a cache someone is counting on
  offline would evaporate exactly when the phone fills up. Purgeable is right
  for a copy nobody asked to keep, so the honest layout is two tiers rather
  than one directory winning — the downstream cache design settled on exactly
  that split. On iOS set
  `isExcludedFromBackup` on it, or a mirrored tree inflates the user's iCloud
  backup — which Apple's storage guidelines treat as a review matter.
- **Save As writes nothing on a phone.** The save sheet returns a `content://`
  URI and `FilePath::into_path()` fails on one, which `save_asked` used to read
  as a cancelled dialog. It now says the feature is not there yet; making it
  real means writing through the descriptor the dialog plugin can hand back, or
  taking a dependency on the fs plugin. Drop the `cfg!(mobile)` arm in
  `save_asked` when it lands. `set_directory(download_dir())` is wrong there
  too — on Android that resolves to the app's own folder wearing the user's
  name for it.
- **A backend that will not hand a file over whole now says so** —
  `Vfs::open_limit`, read by `file_page` before it draws and by `serve_raw`
  before it opens. Past it the page is words rather than a media element
  pointed at bytes that are about to be refused, and a typed URL gets a 413
  saying why instead of a 404 for a file that is right there in the listing.
  What is *not* carried through is an arbitrary `open()` failure: those are
  still a bare 404. A shape for an error with a sentence in it would fix the
  rest, and nothing needs one yet.
- **The safe-area floors are one device's numbers.** The 36/24px top and 18px
  bottom in `VIEWPORT` stand in for WebViews before Chromium 140, which report
  zero insets; left and right are still 0, so a landscape cutout on such a
  WebView eats the pane's edge. Needs measurements from real hardware, not a
  better guess.
- Whatever else the first real mobile build trips over belongs in this item.
  Keep the diff cfg-gated so desktop output is untouched.

## 4. Publish `treeserve` to crates.io

**Trigger:** downstream's browsing layer is done and `Vfs` survived it
unchanged.

Decide the version (0.2.0), re-read `Vfs` / `Meta` / `Entry` / `PaneSection`
for publish-worthiness — crates.io is forever — then `cargo publish -p
treeserve` after a dry run.

**`treestamp` has to go first, or come inside.** `treeserve` depends on it now
(`--version`, the page footer) and it is `publish = false`, so the dry run will
refuse. Either publish it too — it has no dependencies and the manifest already
carries `version = "0.1"` beside the path for exactly this — or fold its half
page of git-asking into `build.rs` and drop the crate from this package. The
first keeps one implementation for all four binaries and is the reason the
version is already written down. Downstream may then swap its submodule for a
version dependency; that is its choice, not a requirement of this item.

## 5. Small cleanups

**Trigger:** convenience. Bundle each with a neighbouring change rather than
making a commit of its own.

- `Serving.entry` is stored as a `String` and re-parsed on every re-root, with
  the parse error swallowed. Store a `tauri::Url`.
- Windows verbatim-only names (a trailing dot or space) lose their meaning
  under display-form probing and in `recent.txt`. Document it in the README as
  a known limitation if anyone ever reports it; actually fixing it means
  carrying verbatim forms end to end, which the RootId design deliberately
  traded away.
- If `clippy` is ever introduced, do it repo-wide in one commit, not piecemeal.
