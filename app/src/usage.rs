//! The pages that say how the program works, and the root that serves them.
//!
//! Compiled in, so they are as current as the build, cannot go missing, and ask
//! nothing of the platform to reach — no folder, no grant, no network. Served
//! through the same seam as any other root, which is what makes them ordinary
//! files to every view in `treeserve`.
//!
//! A program built on this shell contributes pages of its own for the things it
//! does that this one does not, and replaces `index.md`, which is the one page
//! that has to name the program it is about. Whole pages, rather than markers
//! inside a page: what differs between two shells is nearly always a subject,
//! not a paragraph, and a placeholder nobody filled in is visible to the reader.

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use treeserve::embed::EmbeddedFs;
use treeserve::Root;

use crate::{show, Serving, SharedExt, WINDOW};

/// The scheme in the RootId. Not a path, so nothing tries to stat it and
/// `root_id_is_local` answers no.
pub const SCHEME: &str = "usage";

/// The whole set is one root, and this is its id.
pub const ROOT_ID: &str = "usage:/";

/// What the Places row and the window title call it.
pub const LABEL: &str = "Usage";

/// This shell's own pages. Registered by hand rather than swept up from the
/// directory: a file that was forgotten is then visibly absent instead of
/// silently unreachable, and it costs no build script and no dependency.
const PAGES: &[(&str, &[u8])] = &[
    ("index.md", include_bytes!("../usage/index.md")),
    ("keyboard.md", include_bytes!("../usage/keyboard.md")),
    (
        "opening-folders.md",
        include_bytes!("../usage/opening-folders.md"),
    ),
    ("preferences.md", include_bytes!("../usage/preferences.md")),
    (
        "reading-files.md",
        include_bytes!("../usage/reading-files.md"),
    ),
    (
        "tree-and-pane.md",
        include_bytes!("../usage/tree-and-pane.md"),
    ),
];

/// Whether a RootId names the usage tree.
pub fn claims(id: &str) -> bool {
    id == ROOT_ID || id.starts_with("usage:/")
}

/// Serves it. Main thread, and no thread of its own: there is nothing to wait
/// for, which is the point of a root that is already in memory.
///
/// Not remembered in Recent. It is a Place, and the fixed list already shows it —
/// a Place that added itself to Recent would only be duplicating a shortcut.
pub fn open(app: &AppHandle) {
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    let extra = match app.try_state::<SharedExt>() {
        Some(ext) => ext.0.usage_pages.clone(),
        None => Vec::new(),
    };
    let vfs = EmbeddedFs::new(SCHEME, PAGES.iter().copied().chain(extra));
    serving.state().cfg.set_root_vfs(Root {
        id: ROOT_ID.to_string(),
        vfs: Arc::new(vfs),
    });
    // The set has a name of its own; without one the title would be taken from
    // the id, which ends in a slash and names nothing.
    serving.state().cfg.set_root_name(Some(LABEL.to_string()));
    if let Some(win) = app.get_webview_window(WINDOW) {
        if let Ok(url) = serving.entry.parse() {
            let _ = win.navigate(url);
        }
        show(&win);
    }
}
