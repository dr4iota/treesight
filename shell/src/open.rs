//! Hand a URL to the system browser or mailer.
//!
//! The shell's own navigation takes this path for a link out of the tree, and
//! so does an embedder that has a URL with no real `<a>` — an OSC 8 hyperlink
//! in a terminal is cells, not markup. The allowlist lives here so it cannot
//! drift: whoever opens a URL for this window asks this module, not a copy.

use std::thread;

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// Hands a link out of the tree to the system browser or mailer — but only a
/// URL that is actually one of those. The bytes reach here from served content
/// (a link in a hostile README), so the scheme is allowlisted rather than
/// trusted: `http`/`https` to a browser, `mailto` to a mailer, everything else
/// dropped. `javascript:`, `data:`, `file:`, `blob:`, `intent:`, `content:` and
/// the rest are never forwarded, so a README cannot fire an Android Intent,
/// reach a local file, or smuggle a script through the app's own opener. The CSP
/// on `html_reply` already stops such a URL from *running*; this stops the app
/// from *launching* it. On a thread, because on mobile `open_url` is a plugin
/// round trip with no timeout, dispatched to the very thread the callback holds.
pub fn open_externally<R: tauri::Runtime>(app: &AppHandle<R>, url: &tauri::Url) {
    if !may_open_externally(url.scheme()) {
        return;
    }
    let app = app.clone();
    let url = url.clone();
    thread::spawn(move || {
        let _ = app.opener().open_url(url.as_str(), None::<&str>);
    });
}

/// The only schemes handed to the OS. An allowlist, not a denylist: a new
/// dangerous scheme should be refused by default, not remembered to block.
pub fn may_open_externally(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "mailto")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a real page or a mailer leaves for the OS. A link in a hostile
    /// README must not fire an Android Intent, reach a local file, or forward a
    /// scheme that survived the markdown gate.
    #[test]
    fn only_browsers_and_mail_leave_for_the_os() {
        for ok in ["http", "https", "mailto"] {
            assert!(may_open_externally(ok), "{ok}");
        }
        for no in [
            "javascript", "data", "file", "blob", "intent", "content", "about", "ftp", "smb", "ws",
            "wss", "telesight",
        ] {
            assert!(!may_open_externally(no), "{no}");
        }
    }
}
