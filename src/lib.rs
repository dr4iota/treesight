//! Server-rendered file/code tree browser.
//!
//! The binary in `main.rs` is a thin CLI over this library; embedders (the
//! Tauri app in `app/`) call [`spawn`] and drive the server themselves.

pub mod embed;
pub mod hl;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub use http::{spawn, Serving};
pub mod md;
pub mod page;
pub mod util;
pub mod vfs;
pub mod view;

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::UNIX_EPOCH;

use two_face::theme::EmbeddedThemeName;

use hl::Hl;
use page::{Prefs, ThemeMode};
use util::*;
pub use vfs::{Entry, LocalFs, Meta, ReadSeek, ResolveError, Vfs, VfsPath};

/// glibc keeps giving the single-precision math functions new symbol versions
/// — `hypotf` in 2.35, `atan2f` in 2.43 — so a binary built on a host that has
/// them will not start on anything older, and the Mermaid renderer calls both.
/// Define the symbols ourselves, in pure Rust, and the references are
/// satisfied at link time instead of against the build host's libm. Only glibc
/// versions its symbols this way; elsewhere the platform's own definition would
/// collide with ours at link time, so leave those alone.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[unsafe(no_mangle)]
pub extern "C" fn atan2f(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[unsafe(no_mangle)]
pub extern "C" fn hypotf(x: f32, y: f32) -> f32 {
    libm::hypotf(x, y)
}

const APP_CSS: &str = include_str!("app.css");

/// The stylesheet this crate's pages are drawn with, for a shell that serves
/// pages of its own beside them — telesight's Servers editor and terminal, which
/// come off a different scheme and so cannot link `/.ts/app.css`. It carries the
/// palette (`--bg`, `--fg`, `--link`, …), the header, and the label-to-icon
/// collapse the narrow window relies on, so a page that wears it matches without
/// keeping a second copy of any of that in step.
pub fn app_css() -> &'static str {
    APP_CSS
}
const MATH_CSS: &str = include_str!("math.css");

/// What one of the pane's shortcuts turned out to be, once something looked.
///
/// `Unknown` is where every one of them starts, and it is not a failure to know:
/// the lists are rendered before anything is checked, on purpose, because
/// checking a path is a syscall that can block for as long as a network drive
/// takes to give up. An entry says nothing about itself until there is something
/// true to say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RootStatus {
    Unknown,
    Ok,
    /// It answered, and there is nothing there any more.
    Missing,
    /// It did not answer: a drive that is not ready, a share whose host has gone,
    /// a folder we are not allowed to look into.
    Unreachable,
}

impl RootStatus {
    /// What to put next to the entry, or `None` while there is nothing to say.
    pub fn note(self) -> Option<&'static str> {
        match self {
            RootStatus::Unknown | RootStatus::Ok => None,
            RootStatus::Missing => Some("missing"),
            RootStatus::Unreachable => Some("not available"),
        }
    }
}

/// A root the reader pinned: the RootId, and a name for it when the id is not
/// one. Where Recent collects what was opened, this list is only ever added to
/// on purpose — which is why it is separate from Places, whose rows the platform
/// decides, and separate from Recent, whose rows nobody chose.
pub struct Pin {
    pub id: String,
    /// What to call it. `None` draws the id as a path, the way a Recent row is
    /// drawn; a root whose id is not readable — a scheme, a hash, a grant —
    /// carries the name it was pinned under instead.
    pub label: Option<String>,
}

/// The served root: a RootId naming it, and the backend that answers for it.
///
/// A RootId is a scheme-aware string. A local root's id is the bare
/// display-form host path — exactly the string `recent.txt`, the Places list
/// and the `/.ts/root` links have always carried — and a remote backend's id
/// carries a scheme prefix (`ssh:<bookmark>:/path`). A single-letter prefix
/// is a Windows drive letter, not a scheme.
pub struct Root {
    pub id: String,
    pub vfs: Arc<dyn Vfs>,
}

impl Root {
    /// A root on the local filesystem. The path should already be
    /// canonicalized, as every caller of [`Config::new`] has always done.
    pub fn local(root: PathBuf) -> Root {
        let vfs = Arc::new(LocalFs::new(root));
        Root { id: vfs.root_id(), vfs }
    }
}

/// One extra section in the side pane, rendered between Places and Recent.
///
/// Everything in it is inert markup: links a shell intercepts, exactly like
/// Places, and this server has no route that would act on any of them. That is
/// what makes a section safe to draw — the list is somebody else's to know
/// about, and drawing it is all this side does. A section with no entries
/// renders nothing, so a downstream list can start out empty.
#[derive(Clone)]
pub struct PaneSection {
    /// CSS hook on the `<section>`, alongside the built-in `places` and
    /// `recent`.
    pub class: String,
    pub heading: String,
    /// An action on the heading bar itself — "add a server". Drawn as a plus,
    /// because a bar that narrow has room for one thing and adding one more of
    /// what the section lists is that thing: (href, title).
    /// A control on the heading: where it goes, the mark it draws, and what it
    /// says. The mark is the embedder's because the action is — a list of servers
    /// is *managed*, and a plus promises adding one.
    pub heading_action: Option<(String, String, String)>,
    pub entries: Vec<PaneEntry>,
}

/// One row of a [`PaneSection`].
#[derive(Clone)]
pub struct PaneEntry {
    /// What to call it. `None` draws the id as a path, the way Recent does.
    pub label: Option<String>,
    /// The RootId this row names. Its note comes from [`Config::root_status`],
    /// like every other entry in the pane.
    pub id: String,
    /// Where the row goes: `{action}?path={id}` percent-encoded, the shape
    /// `/.ts/place` and `/.ts/root` already have.
    pub action: String,
    /// Smaller links on the row, shown on hover like the tree's re-root
    /// button: (href, svg icon paths, title).
    pub aside: Vec<(String, String, String)>,
}

pub struct Config {
    /// The served root: its identity and the backend that answers for it.
    /// Behind a lock so an embedder can re-root a running server; the CLI
    /// never changes it.
    root: RwLock<Option<Arc<Root>>>,
    /// Site title. `None` means "name of the served directory", which follows
    /// the root when it changes.
    title: Option<String>,
    /// Where the web server listens. Nothing else in here reads them: a caller
    /// driving [`handle`] itself has no address to bind and no pool to size.
    #[cfg(feature = "http")]
    pub bind: String,
    #[cfg(feature = "http")]
    pub port: u16,
    pub theme: ThemeMode,
    pub ln: bool,
    pub sidebar: bool,
    pub show_hidden: bool,
    #[cfg(feature = "http")]
    pub threads: usize,
    pub syn_light: EmbeddedThemeName,
    pub syn_dark: EmbeddedThemeName,
    /// Renders the controls that only make sense inside the desktop shell: the
    /// path bar, Back/Forward, the Places and Recent lists and the button that
    /// opens the native folder picker. Every one of them re-roots the server or
    /// names a path outside it, so this stays off for anything served over a
    /// network and there is deliberately no CLI flag to turn it on. The
    /// controls do nothing on their own: they are links the shell intercepts,
    /// and this server has no route that would act on them.
    pub app_ui: bool,
    /// Whether this platform can be asked for a folder. Desktop can; a phone's
    /// picker wants a per-directory grant the shell does not request yet, so the
    /// start page offers what it can already reach instead of a button that does
    /// nothing when pressed.
    pub picker: bool,
    /// What the product is called, for the one page that has no folder to be
    /// named after. The embedder's name, not this crate's: a shell serving these
    /// pages is what the reader thinks they are using.
    pub app_name: Option<String>,
    /// The version to print beside [`Config::app_name`]. Same reason: a shell
    /// embedding this crate ships on its own version, not on this one's.
    pub app_version: Option<String>,
    /// What the start page says this program is. Plain text, escaped by the page;
    /// `None` uses the sentence this crate would write about itself, which is only
    /// right for the program this crate is.
    pub intro: Option<String>,
    /// Fixed shortcuts for the Places list — (label, RootId) pairs the
    /// embedder supplies, since it is the side that knows the platform's
    /// home, desktop and drive layout. Only rendered when `app_ui` is set.
    pub places: Vec<(String, String)>,
    /// Recently served roots as RootIds, newest first. Behind a lock like
    /// `root`, since it grows while the server runs.
    recent: RwLock<Arc<Vec<String>>>,
    /// Roots the reader pinned, in the order they were pinned. Behind a lock for
    /// the reason `recent` is: pinning happens while the server runs, and the
    /// next page render is where it shows up.
    pinned: RwLock<Arc<Vec<Pin>>>,
    /// What to call the current root in a title. Behind a lock like `root`,
    /// because it is set when the root is, from whatever re-rooted.
    root_name: RwLock<Option<String>>,
    /// Extra pane sections from whoever embedded this server. Behind a lock
    /// for the reason `recent` is: a config UI can add a server while the
    /// server is running, and the next page render is where that shows up.
    sections: RwLock<Arc<Vec<PaneSection>>>,
    /// What the Places and Recent paths turned out to be, for the ones anything
    /// has got round to looking at. Written by the embedder as its answers come
    /// in and read while a page renders, which is the whole point of it being
    /// separate from the two lists: they go out immediately and this catches up.
    status: RwLock<HashMap<String, RootStatus>>,
}

impl Config {
    /// Config with library defaults, matching the CLI's own defaults.
    pub fn new(root: PathBuf) -> Config {
        let cfg = Config::rootless();
        cfg.set_root(root);
        cfg
    }

    /// A server with nothing open yet.
    ///
    /// The shell starts here now: it opens a folder when it is given one and
    /// otherwise shows what there is to open, rather than putting a modal picker
    /// in front of a window nobody has seen. Every page that needs a root asks
    /// for one and finds `None`; `handle` answers those with the start page.
    pub fn rootless() -> Config {
        Config {
            root: RwLock::new(None),
            title: None,
            #[cfg(feature = "http")]
            bind: "127.0.0.1".to_string(),
            #[cfg(feature = "http")]
            port: 8080,
            theme: ThemeMode::Auto,
            ln: true,
            sidebar: true,
            show_hidden: false,
            #[cfg(feature = "http")]
            threads: 8,
            syn_light: EmbeddedThemeName::InspiredGithub,
            syn_dark: EmbeddedThemeName::OneHalfDark,
            app_ui: false,
            picker: false,
            app_name: None,
            app_version: None,
            intro: None,
            places: Vec::new(),
            recent: RwLock::new(Arc::new(Vec::new())),
            pinned: RwLock::new(Arc::new(Vec::new())),
            root_name: RwLock::new(None),
            sections: RwLock::new(Arc::new(Vec::new())),
            status: RwLock::new(HashMap::new()),
        }
    }

    /// The root being served, if one is. `None` is a state, not a failure: it is
    /// what a window looks like before anybody has chosen a folder.
    pub fn root(&self) -> Option<Arc<Root>> {
        self.root.read().expect("root lock").clone()
    }

    /// Re-roots a running server onto a local directory. The path should
    /// already be canonicalized.
    pub fn set_root(&self, root: PathBuf) {
        self.set_root_vfs(Root::local(root));
    }

    /// Re-roots a running server onto any backend. Id and backend travel
    /// together, atomically — a page renders one root or the other, never a
    /// title from one and a tree from another.
    ///
    /// The root's name goes with the root it named. A caller with a better one
    /// says so after this, which is the only way a name outlives a re-root — or
    /// the window would still be called after the folder before it.
    /// Back to nothing open — the state the shell starts in, and the one the start
    /// page is for. The root's name goes with the root, as on any re-root.
    pub fn close_root(&self) {
        self.set_root_name(None);
        *self.root.write().expect("root lock") = None;
    }

    pub fn set_root_vfs(&self, root: Root) {
        self.set_root_name(None);
        *self.root.write().expect("root lock") = Some(Arc::new(root));
    }

    pub fn recent(&self) -> Arc<Vec<String>> {
        Arc::clone(&self.recent.read().expect("recent lock"))
    }

    /// Replaces the Recent list, newest first. Called by the embedder whenever
    /// it re-roots, so the next page render shows the new order.
    pub fn set_recent(&self, recent: Vec<String>) {
        *self.recent.write().expect("recent lock") = Arc::new(recent);
    }

    pub fn pinned(&self) -> Arc<Vec<Pin>> {
        Arc::clone(&self.pinned.read().expect("pinned lock"))
    }

    /// Replaces the pinned list. Called by the embedder at start and again
    /// whenever it pins or unpins something.
    pub fn set_pinned(&self, pinned: Vec<Pin>) {
        *self.pinned.write().expect("pinned lock") = Arc::new(pinned);
    }

    /// Whether `id` is pinned — what the header's own control reads to know
    /// which way round it is.
    pub fn is_pinned(&self, id: &str) -> bool {
        self.pinned().iter().any(|p| p.id == id)
    }

    pub fn sections(&self) -> Arc<Vec<PaneSection>> {
        Arc::clone(&self.sections.read().expect("sections lock"))
    }

    /// Replaces the extra pane sections. Called by the embedder at start and
    /// again whenever its own list changes; the next page render shows it.
    pub fn set_sections(&self, sections: Vec<PaneSection>) {
        *self.sections.write().expect("sections lock") = Arc::new(sections);
    }

    /// What a shortcut turned out to be. `Unknown` for anything nobody has
    /// looked at, which a page renders as an ordinary entry.
    pub fn root_status(&self, id: &str) -> RootStatus {
        self.status
            .read()
            .expect("status lock")
            .get(id)
            .copied()
            .unwrap_or(RootStatus::Unknown)
    }

    /// Records what a shortcut turned out to be. Every page rendered after this
    /// shows it; the one already on screen was static when it left and stays
    /// that way, which is the trade for having no script in it.
    pub fn set_root_status(&self, id: String, status: RootStatus) {
        self.status.write().expect("status lock").insert(id, status);
    }

    /// The site title as of now. Rootless, that is the product's own name — there
    /// is no folder to be named after yet.
    /// `name vX.Y.Z` for the status line, from whatever this program is rather
    /// than from this crate.
    pub fn app_label(&self) -> String {
        format!(
            "{} v{}",
            self.app_name
                .clone()
                .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string()),
            self.app_version
                .clone()
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
        )
    }

    pub fn title(&self) -> String {
        match self.root() {
            Some(root) => self.title_for(&root),
            None => self
                .title
                .clone()
                .or_else(|| self.app_name.clone())
                .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string()),
        }
    }

    /// The site title for a root the caller already holds — so one request
    /// renders one root everywhere, instead of re-reading the lock and
    /// racing a re-root between its own header and its own pane.
    pub fn title_for(&self, root: &Root) -> String {
        match &self.title {
            Some(t) => t.clone(),
            None => match leaf_of(&root.id) {
                Some(n) => n.to_string(),
                // A drive, a share or `/` has no last component to show, so
                // name it by the whole id rather than by nothing.
                None => root.id.clone(),
            },
        }
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    /// What to *call* the root, where a name is wanted rather than a path: the
    /// document title and the window's. An embedder that knows the root by a name
    /// of its own — a bookmark's label — says so here, and the header still shows
    /// the folder and the id, which are the two things a title has no room for.
    pub fn root_name(&self) -> Option<String> {
        self.root_name.read().expect("root name lock").clone()
    }

    pub fn set_root_name(&self, name: Option<String>) {
        *self.root_name.write().expect("root name lock") = name;
    }
}

pub struct State {
    pub cfg: Config,
    pub hl: Hl,
}

/// Last path component of a RootId, `None` when there is none to take — a
/// filesystem root, a bare drive, a UNC share root. Mirrors what
/// `Path::file_name` answered when the id was a `PathBuf`, and answers for
/// remote ids too (`ssh:x:/var/www` → `www`). Public so an embedder titles
/// its window by the same rule a page titles itself.
pub fn leaf_of(id: &str) -> Option<&str> {
    // Separators are the host's. A Unix file may legally carry `\` in its
    // name, so only Windows splits on it — which is also what `Path` did.
    #[cfg(windows)]
    const SEPS: &[char] = &['/', '\\'];
    #[cfg(not(windows))]
    const SEPS: &[char] = &['/'];
    // A UNC share root (`\\server\share`) has no component below the root,
    // like `/` and `C:\`, even though it has separators to find.
    #[cfg(windows)]
    if let Some(rest) = id.strip_prefix(r"\\") {
        if rest.matches(SEPS).count() <= 1 {
            return None;
        }
    }
    match id.rfind(SEPS) {
        Some(i) if i + 1 < id.len() => Some(&id[i + 1..]),
        Some(_) => None,
        None if id.is_empty() => None,
        None => Some(id),
    }
}

/// Whether a RootId names a local path — something `PathBuf` and `fs` can be
/// pointed at. A scheme prefix marks a remote id: at least two characters,
/// leading letter, then letters, digits, `+`, `.` or `-` (the URI scheme
/// grammar, so `s3:` counts). A single letter before `:` is a Windows drive.
pub fn root_id_is_local(id: &str) -> bool {
    match id.find(':') {
        Some(i) if i >= 2 => {
            let scheme = &id[..i];
            !(scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')))
        }
        _ => true,
    }
}

/// The middle field of a remote RootId — `ssh:iota:/home/x` → `iota`.
///
/// Which backend it is and what the id means are the embedder's business; the
/// *shape* is this crate's, because it is what `root_id_is_local` and `leaf_of`
/// already read. The header draws it beside the path so a listing says which
/// machine it is from, which is the one thing a remote path cannot say itself:
/// `/home/hanhua` looks the same on every host in the world.
pub fn root_id_bookmark(id: &str) -> Option<&str> {
    if root_id_is_local(id) {
        return None;
    }
    let rest = &id[id.find(':')? + 1..];
    let end = rest.find(':')?;
    Some(&rest[..end]).filter(|s| !s.is_empty())
}

/// Where a URL path points inside the served root.
pub struct Resolved {
    /// Percent-decoded path segments, for breadcrumbs and file names.
    pub rel: Vec<String>,
    /// Canonical path inside the root's backend: symlinks resolved,
    /// confinement checked.
    pub path: VfsPath,
}

/// Why a URL path does not name something servable. The segments are carried
/// along where they are safe to echo back in a page.
pub enum PathError {
    /// Malformed or traversing path; nothing safe to show.
    Bad,
    Missing(Vec<String>),
    Outside(Vec<String>),
}

/// Resolves a URL path against the served root.
///
/// Shared by the request handler and by embedders that need the file behind a
/// link (the desktop app's download handler), so the traversal and
/// symlink-escape checks live in exactly one place.
pub fn resolve_in_root(vfs: &dyn Vfs, url_path: &str) -> Result<Resolved, PathError> {
    let decoded = percent_decode(url_path);
    let rel: Vec<String> = decoded
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if decoded.contains('\0') || rel.iter().any(|s| s == "." || s == "..") {
        return Err(PathError::Bad);
    }

    match vfs.resolve(&VfsPath::new(rel.clone())) {
        Ok(path) => Ok(Resolved { rel, path }),
        Err(ResolveError::Missing) => Err(PathError::Missing(rel)),
        Err(ResolveError::Outside) => Err(PathError::Outside(rel)),
    }
}

/// The shared state a router needs, built from a configuration.
///
/// Building the highlighter is the expensive part of starting, and both faces
/// of this crate need it: the web server on its way up, and an app that means
/// to call [`handle`] itself over no socket at all.
pub fn state_for(cfg: Config) -> Arc<State> {
    let hl = Hl::new(cfg.syn_light, cfg.syn_dark);
    Arc::new(State { cfg, hl })
}

/// A request, as the part of this that decides what to answer sees it.
///
/// Owns its strings rather than borrowing them. tiny_http lends them from a
/// socket it is still holding; a webview's protocol handler has no socket and
/// no lifetime to lend from. Copying a handful of small headers is what buys
/// the same routing code two callers.
pub struct Req {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub is_get: bool,
}

impl Req {
    /// First header with this name, matched case-insensitively as HTTP means it.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.named(name).next()
    }

    /// Every header with this name. `Cookie` may legitimately arrive more than
    /// once, and taking only the first would silently drop preferences.
    fn named<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> + 'a {
        // Owned, so the returned iterator outlives the name it was asked for.
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .filter(move |(k, _)| k.eq_ignore_ascii_case(&name))
            .map(|(_, v)| v.as_str())
    }
}

/// What a reply carries. `Stream` is a handle the caller pumps rather than a
/// buffer we filled: a film is served without being read.
pub enum Body {
    Empty,
    Text(String),
    Stream {
        reader: Box<dyn Read + Send>,
        len: u64,
    },
}

/// An answer, decided but not yet written. Whoever asked decides how it goes
/// out — down a socket, or back through a webview.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

fn hdr(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

impl Reply {
    fn empty(status: u16) -> Reply {
        Reply {
            status,
            headers: Vec::new(),
            body: Body::Empty,
        }
    }

    fn text(status: u16, ctype: &str, body: impl Into<String>) -> Reply {
        Reply {
            status,
            headers: vec![hdr("Content-Type", ctype)],
            body: Body::Text(body.into()),
        }
    }

    fn plain(status: u16, body: &str) -> Reply {
        Reply::text(status, "text/plain; charset=utf-8", body)
    }

    fn with(mut self, k: &str, v: &str) -> Reply {
        self.headers.push(hdr(k, v));
        self
    }
}

fn html_reply(status: u16, body: String) -> Reply {
    Reply::text(status, "text/html; charset=utf-8", body)
        .with("X-Content-Type-Options", "nosniff")
        // Every page is a snapshot of a directory taken when it was asked for,
        // and none of it is worth keeping: what a listing says is true of a
        // moment, and which listing you get at all depends on cookies and on the
        // root the shell currently serves. Saying nothing here left the browser
        // to invent a lifetime of its own, and the one thing a Refresh button
        // must not do is hand back the page it was pressed on.
        .with("Cache-Control", "no-store")
}

/// A stylesheet, tagged with which stylesheet it is.
///
/// The pages are `no-store` and the rules they are laid out by used to be good
/// for five minutes without asking. That is a window in which a rebuilt binary
/// serves its new markup to a browser still holding the old rules, and a page
/// that is half of each is a page nobody wrote — which is exactly how long it
/// takes to conclude that a change did not work. `no-cache` is "ask first", not
/// "keep nothing": the copy stays where it is, and a load spends one small
/// conditional request finding out whether it is still the right one.
fn css_reply(req: &Req, body: &str) -> Reply {
    let tag = text_etag(body);
    if req
        .header("If-None-Match")
        .is_some_and(|given| etag_matches(given, &tag))
    {
        return Reply::empty(304)
            .with("ETag", &tag)
            .with("Cache-Control", "no-cache");
    }
    Reply::text(200, "text/css; charset=utf-8", body)
        .with("Cache-Control", "no-cache")
        .with("ETag", &tag)
}

/// A tag for a stylesheet we hold in memory: what it says, not when it was
/// written. Nothing here is a secret and nothing is defending against a forgery,
/// so the cheapest hash in the standard library is the right one.
fn text_etag(body: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    format!("\"{:x}\"", hasher.finish())
}

fn prefs_from<'a>(state: &State, req: &Req, open: &'a [String]) -> Prefs<'a> {
    let mut prefs = Prefs {
        theme: state.cfg.theme,
        ln: state.cfg.ln,
        sidebar: state.cfg.sidebar,
        open,
    };
    for value in req.named("Cookie") {
        for (k, v) in parse_cookies(value) {
            match k.as_str() {
                "ts_theme" => {
                    if let Some(t) = ThemeMode::from_str(&v) {
                        prefs.theme = t;
                    }
                }
                "ts_ln" => prefs.ln = v == "1",
                "ts_sidebar" => prefs.sidebar = v == "1",
                _ => {}
            }
        }
    }
    prefs
}

fn wants_html(req: &Req) -> bool {
    req.header("Accept")
        .map(|a| a.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
}

/// Subresource loads (e.g. <img src="relative.png"> inside rendered
/// markdown) must get raw bytes even without ?raw=1.
fn is_subresource(req: &Req) -> bool {
    matches!(
        req.header("Sec-Fetch-Dest"),
        Some("image" | "video" | "audio" | "embed" | "object" | "font")
    )
}

/// Decides the answer to one request, touching no socket and no webview.
///
/// This is the whole of treeserve as far as a caller is concerned: the HTTP
/// server pumps it, and so can anything else holding a [`Req`].
pub fn handle(state: &State, req: &Req) -> Reply {
    if !req.is_get {
        return Reply::plain(405, "method not allowed");
    }

    let url_now = req.url.clone();
    let (path_raw, query_raw) = match url_now.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (url_now.clone(), String::new()),
    };
    let query = parse_query(&query_raw);
    // Owned here so `Prefs` can borrow it and stay `Copy` for every renderer.
    let open = open_from(req);
    let prefs = prefs_from(state, req, &open);
    // One snapshot of the served root for the whole request, so a re-root
    // mid-request cannot hand half a page one tree and half another.
    let root = state.cfg.root();

    // Internal routes (assets + preference cookies).
    match path_raw.as_str() {
        "/.ts/app.css" => return css_reply(req, APP_CSS),
        "/.ts/math.css" => return css_reply(req, MATH_CSS),
        "/.ts/syntax-light.css" => return css_reply(req, &state.hl.css_light),
        "/.ts/syntax-dark.css" => return css_reply(req, &state.hl.css_dark),
        "/.ts/set" => return set_prefs(&query),
        "/.ts/tree" => return toggle_open(req, &query),
        // Somewhere for the shell to park the window while it finds out whether a
        // folder opens. Only it ever navigates here, and only when `app_ui` is on;
        // served on its own this is a page about nothing.
        "/.ts/wait" if state.cfg.app_ui => {
            let path = query_get(&query, "path").unwrap_or_default();
            return html_reply(
                200,
                page::wait_page(state, root.as_deref(), prefs, &url_now, path),
            );
        }
        _ => {}
    }

    // Nothing open. The start page says what there is to open; every other path
    // is a URL from before there was nothing — history, a bookmark, a reload
    // after the folder was closed — and goes to the one page that exists rather
    // than to an error about a root that is not the point.
    let Some(root) = root else {
        if path_raw == "/" {
            return html_reply(200, page::start_page(state, prefs, &url_now));
        }
        return Reply::empty(302).with("Location", "/");
    };

    // Decode and sanitize the served path.
    let (rel, canon) = match resolve_in_root(root.vfs.as_ref(), &path_raw) {
        Ok(r) => (r.rel, r.path),
        Err(PathError::Bad) => {
            return html_reply(
                400,
                page::error_page(state, &root, prefs, &[], &url_now, 400, "bad path"),
            );
        }
        Err(PathError::Missing(rel)) => {
            return html_reply(
                404,
                page::error_page(state, &root, prefs, &rel, &url_now, 404, "not found"),
            );
        }
        Err(PathError::Outside(rel)) => {
            return html_reply(
                403,
                page::error_page(state, &root, prefs, &rel, &url_now, 403, "forbidden"),
            );
        }
    };

    let is_dir = root
        .vfs
        .metadata(&canon)
        .map(|m| m.is_dir)
        .unwrap_or(false);
    if is_dir {
        // Directory URLs need a trailing slash so relative links resolve.
        if !path_raw.ends_with('/') {
            let loc = if query_raw.is_empty() {
                format!("{}/", path_raw)
            } else {
                format!("{}/?{}", path_raw, query_raw)
            };
            return Reply::empty(301).with("Location", &loc);
        }
        return if wants_html(req) {
            html_reply(
                200,
                page::listing_page(state, &root, prefs, &rel, &canon, &query, &url_now),
            )
        } else {
            Reply::plain(200, &page::listing_text(state, root.vfs.as_ref(), &canon))
        };
    }

    let name = rel.last().cloned().unwrap_or_default();
    let want_raw = query_get(&query, "raw") == Some("1");
    let want_dl = query_get(&query, "dl") == Some("1");
    // A file served as itself arrives with nothing around it: no path, no Back,
    // and in the shell — which has no chrome of its own — no way out at all but a
    // keystroke there is nothing on screen to suggest. So the raw view is the
    // file in a frame with the page's own chrome around it, in the shell and in a
    // browser alike, because the file is the same file in both. The frame asks
    // for it with `bare` and gets it, as does every request that did not ask for
    // HTML in the first place: every tool, and every `<img>`, `<video>` and
    // `<embed>` we write. Nothing that fetches a file rather than looks at one
    // sees this page.
    let bare = query_get(&query, "bare") == Some("1");
    let framed = want_raw && !want_dl && !bare;
    if framed && wants_html(req) && !is_subresource(req) {
        return html_reply(200, view::raw_page(state, &root, prefs, &rel, &url_now));
    }
    if want_raw || want_dl || !wants_html(req) || is_subresource(req) {
        return serve_raw(req, root.vfs.as_ref(), &canon, &name, want_dl);
    }

    html_reply(
        200,
        view::file_page(state, &root, prefs, &rel, &canon, &query, &url_now),
    )
}

fn cookie_header(k: &str, v: &str) -> (String, String) {
    hdr(
        "Set-Cookie",
        &format!("{}={}; Path=/; Max-Age=31536000; SameSite=Lax", k, v),
    )
}

/// Local redirect target from `?back=`, defaulting to the root.
fn back_target(query: &[(String, String)]) -> String {
    query_get(query, "back")
        .filter(|b| b.starts_with('/') && !b.starts_with("//"))
        .unwrap_or("/")
        .to_string()
}

/// How many directories the pane will hold open at once, and how many bytes of
/// cookie that is allowed to cost.
///
/// Every one of them is a `read_dir` on every render — a network round trip where
/// the backend is remote — so this is a budget, not a limit anybody should reach.
/// The byte cap is the one that actually binds: paths are long, cookies are a few
/// KB, and a cookie a browser quietly drops is a pane that forgets everything.
const OPEN_MAX: usize = 24;
const OPEN_COOKIE_MAX: usize = 3000;

/// The `ts_open` cookie: relative paths, percent-encoded, `|`-joined.
///
/// `|` needs no meaning of its own — `percent_encode` leaves only unreserved
/// characters alone, so a path can never contain the separator.
fn open_from(req: &Req) -> Vec<String> {
    for value in req.named("Cookie") {
        for (k, v) in parse_cookies(value) {
            if k == "ts_open" {
                return v.split('|').filter_map(open_path).take(OPEN_MAX).collect();
            }
        }
    }
    Vec::new()
}

/// One entry, as the tree keys them: percent-decoded, no leading or trailing
/// slash, and nothing that could climb out of the root. `resolve_in_root` guards
/// what gets *served*; this guards what gets walked to draw the pane.
fn open_path(raw: &str) -> Option<String> {
    let path = percent_decode(raw.trim()).trim_matches('/').to_string();
    let sane = !path.is_empty()
        && !path.contains('\0')
        && !path
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..");
    sane.then_some(path)
}

/// The cookie value, oldest entries dropped until it fits. Newest last, so what
/// goes is what was opened longest ago.
fn open_cookie(open: &[String]) -> String {
    let mut from = 0;
    loop {
        let value = open[from..]
            .iter()
            .map(|p| percent_encode(p))
            .collect::<Vec<_>>()
            .join("|");
        if value.len() <= OPEN_COOKIE_MAX || from + 1 >= open.len() {
            return value;
        }
        from += 1;
    }
}

/// `/.ts/tree?open=src/bin&back=/src/` — and `shut=` for the same trip the other
/// way. Read the set, change one entry, write it back: the direction is in the
/// link because a plain toggle is wrong the second time it is followed, which is
/// exactly what a double click and a reload both do.
fn toggle_open(req: &Req, query: &[(String, String)]) -> Reply {
    let mut open = open_from(req);
    if let Some(path) = query_get(query, "shut").and_then(open_path) {
        open.retain(|p| *p != path);
    }
    if let Some(path) = query_get(query, "open").and_then(open_path)
        && !open.contains(&path)
    {
        // The oldest goes rather than the newest being refused: the reader just
        // asked for this one, and something has to leave.
        if open.len() >= OPEN_MAX {
            open.remove(0);
        }
        open.push(path);
    }
    let mut reply = Reply::empty(303);
    reply
        .headers
        .push(cookie_header("ts_open", &open_cookie(&open)));
    reply.with("Location", &back_target(query))
}

/// `/.ts/set?theme=dark&ln=1&sidebar=0&back=/some/where` — store prefs in
/// cookies and bounce back. Pure SSR option switching, no JS.
fn set_prefs(query: &[(String, String)]) -> Reply {
    let mut reply = Reply::empty(303);
    for (k, v) in query {
        match k.as_str() {
            "theme" if ThemeMode::from_str(v).is_some() => {
                reply.headers.push(cookie_header("ts_theme", v));
            }
            "ln" if v == "0" || v == "1" => {
                reply.headers.push(cookie_header("ts_ln", v));
            }
            "sidebar" if v == "0" || v == "1" => {
                reply.headers.push(cookie_header("ts_sidebar", v));
            }
            _ => {}
        }
    }
    reply.with("Location", &back_target(query))
}

/// Parse "bytes=a-b" | "bytes=a-" | "bytes=-n". Multi-range is ignored
/// (the whole file is served instead, which is allowed by RFC 9110).
fn parse_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let spec = value.strip_prefix("bytes=")?.trim();
    if spec.contains(',') || size == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let end_default = size - 1;
    match (a.is_empty(), b.is_empty()) {
        (false, true) => Some((a.parse().ok()?, end_default)),
        (false, false) => {
            let s: u64 = a.parse().ok()?;
            let e: u64 = b.parse().ok()?;
            Some((s, e.min(end_default)))
        }
        (true, false) => {
            let n: u64 = b.parse().ok()?;
            Some((size.saturating_sub(n), end_default))
        }
        (true, true) => None,
    }
}

/// What the file is right now, as a tag a browser can quote back at us.
///
/// Modification time and length rather than a hash of the contents: reading the
/// whole file to decide whether to send the whole file costs what sending it
/// costs. The nanoseconds are in there because a second is a long time in the
/// life of a file being edited, and two writes inside one would otherwise share
/// a tag.
fn etag_for(meta: &Meta) -> Option<String> {
    let t = meta.mtime?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!(
        "\"{:x}-{:x}-{:x}\"",
        t.as_secs(),
        t.subsec_nanos(),
        meta.len
    ))
}

/// Whether an `If-None-Match` header names the tag we would send. It may hold a
/// list, each entry may be marked weak, and `*` means "anything you have".
fn etag_matches(given: &str, tag: &str) -> bool {
    let given = given.trim();
    given == "*"
        || given
            .split(',')
            .map(str::trim)
            .any(|t| t.strip_prefix("W/").unwrap_or(t) == tag)
}

fn serve_raw(req: &Req, vfs: &dyn Vfs, path: &VfsPath, name: &str, attachment: bool) -> Reply {
    let Ok(mut file) = vfs.open(path) else {
        return Reply::plain(404, "not found");
    };
    // The size of the handle being streamed, not of whatever the path names
    // by the time a second lookup runs — a file replaced between the two
    // would otherwise get the new length on the old bytes. The ETag still
    // comes from a path lookup, so it is only kept when it describes the
    // same length as the handle.
    let size = file
        .seek(SeekFrom::End(0))
        .and_then(|n| file.seek(SeekFrom::Start(0)).map(|_| n))
        .unwrap_or(0);
    let meta = vfs.metadata(path).ok().filter(|m| m.len == size);
    let etag = meta.as_ref().and_then(etag_for);
    let mime = mime_for_ext(&ext_of(name));

    // Reloading a page does not reload what is inside it: the picture, the video
    // and the PDF are separate requests, and with nothing to check them against
    // the browser kept the copies it had — so a refreshed page went on painting
    // the old image. `no-cache` is "ask first", not "keep nothing": the copy
    // stays where it is and a reload spends one small conditional request per
    // file finding out whether it is still good.
    let mut headers = vec![
        hdr("Content-Type", mime),
        hdr("Accept-Ranges", "bytes"),
        hdr("X-Content-Type-Options", "nosniff"),
        hdr(
            "Cache-Control",
            if etag.is_some() { "no-cache" } else { "no-store" },
        ),
    ];
    if let Some(tag) = &etag {
        headers.push(hdr("ETag", tag));
    }

    // It asked, and the answer is that nothing has changed. Not for a range
    // request: that client is part-way through a file it already holds, and a
    // 304 answers a question it did not ask.
    if let Some(tag) = &etag
        && req.header("Range").is_none()
        && req
            .header("If-None-Match")
            .is_some_and(|given| etag_matches(given, tag))
    {
        return Reply::empty(304)
            .with("ETag", tag)
            .with("Cache-Control", "no-cache");
    }
    if attachment {
        let safe: String = name.chars().filter(|c| *c != '"' && *c != '\\').collect();
        headers.push(hdr(
            "Content-Disposition",
            &format!("attachment; filename=\"{}\"", safe),
        ));
    }

    let range = req.header("Range").and_then(|v| parse_range(v, size));
    match range {
        Some((start, end)) if start < size && start <= end => {
            if file.seek(SeekFrom::Start(start)).is_err() {
                return Reply::plain(500, "io error");
            }
            let len = end - start + 1;
            headers.push(hdr(
                "Content-Range",
                &format!("bytes {}-{}/{}", start, end, size),
            ));
            Reply {
                status: 206,
                headers,
                body: Body::Stream {
                    reader: Box::new(file.take(len)),
                    len,
                },
            }
        }
        Some(_) => Reply::plain(416, "range not satisfiable")
            .with("Content-Range", &format!("bytes */{}", size)),
        None => Reply {
            status: 200,
            headers,
            body: Body::Stream {
                reader: file,
                len: size,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{leaf_of, open_cookie, open_from, open_path, root_id_is_local, toggle_open, Req};
    use super::{parse_query, OPEN_MAX};

    fn req(cookie: &str) -> Req {
        Req {
            url: "/".to_string(),
            headers: match cookie.is_empty() {
                true => Vec::new(),
                false => vec![("Cookie".to_string(), format!("ts_open={cookie}"))],
            },
            is_get: true,
        }
    }

    fn set_cookie(reply: &super::Reply) -> String {
        reply
            .headers
            .iter()
            .find(|(k, _)| k == "Set-Cookie")
            .map(|(_, v)| v.split(';').next().unwrap_or_default().to_string())
            .expect("a cookie")
    }

    /// What may be walked to draw the pane. `resolve_in_root` guards what is
    /// served; this list is walked before any of that runs.
    #[test]
    fn an_open_entry_cannot_climb_out_of_the_root() {
        assert_eq!(open_path("src/bin"), Some("src/bin".to_string()));
        assert_eq!(open_path("/src/"), Some("src".to_string()));
        assert_eq!(open_path("a%20b"), Some("a b".to_string()));
        assert_eq!(open_path(".."), None);
        assert_eq!(open_path("src/../../etc"), None);
        assert_eq!(open_path("src//bin"), None);
        assert_eq!(open_path("  "), None);
    }

    #[test]
    fn the_open_set_survives_the_cookie() {
        let open = vec!["a b".to_string(), "c/d".to_string()];
        let value = open_cookie(&open);
        assert_eq!(value, "a%20b|c%2Fd");
        assert_eq!(open_from(&req(&value)), open);
    }

    /// One entry changes per trip, and the direction is in the link so following
    /// it twice — a double click, a reload — lands in the same place.
    #[test]
    fn a_trip_through_the_tree_route_changes_one_entry() {
        let open = |q: &str, cookie: &str| {
            let reply = toggle_open(&req(cookie), &parse_query(q));
            assert_eq!(reply.status, 303);
            (set_cookie(&reply), reply)
        };

        let (cookie, reply) = open("open=src&back=/here/", "");
        assert_eq!(cookie, "ts_open=src");
        assert!(reply.headers.contains(&("Location".to_string(), "/here/".to_string())));
        // Again, same answer: nothing is toggled off by being asked for twice.
        assert_eq!(open("open=src&back=/", "src").0, "ts_open=src");
        assert_eq!(open("open=docs/img&back=/", "src").0, "ts_open=src|docs%2Fimg");
        assert_eq!(open("shut=src&back=/", "src|docs%2Fimg").0, "ts_open=docs%2Fimg");
        // A path that could climb out is not stored, and does not disturb the set.
        assert_eq!(open("open=../etc&back=/", "src").0, "ts_open=src");
    }

    /// The set is a budget: every entry is a `read_dir` per render. What goes is
    /// what was opened longest ago, because the newest is the one just asked for.
    #[test]
    fn the_oldest_open_directory_makes_room_for_a_new_one() {
        let full: Vec<String> = (0..OPEN_MAX).map(|i| format!("d{i}")).collect();
        let reply = toggle_open(&req(&open_cookie(&full)), &parse_query("open=late&back=/"));
        let stored = open_from(&req(set_cookie(&reply).trim_start_matches("ts_open=")));
        assert_eq!(stored.len(), OPEN_MAX);
        assert_eq!(stored.first().map(String::as_str), Some("d1"));
        assert_eq!(stored.last().map(String::as_str), Some("late"));
    }

    /// `leaf_of` must answer what `Path::file_name` answered when a RootId
    /// was a `PathBuf`, across the shapes a canonicalized root can take.
    /// With nothing open, one page exists and every other path leads to it — a
    /// URL from before the folder was closed is history, not an error.
    #[test]
    fn nothing_open_means_one_page_and_a_way_back_to_it() {
        let state = super::state_for(super::Config::rootless());
        let get = |url: &str| {
            super::handle(
                &state,
                &super::Req {
                    url: url.to_string(),
                    headers: vec![("Accept".to_string(), "text/html".to_string())],
                    is_get: true,
                },
            )
        };

        let start = get("/");
        assert_eq!(start.status, 200);
        match start.body {
            super::Body::Text(t) => assert!(t.contains("class=\"app nothing\""), "{t}"),
            _ => panic!("a page is text"),
        }

        let stale = get("/src/main.rs");
        assert_eq!(stale.status, 302);
        assert!(stale
            .headers
            .contains(&("Location".to_string(), "/".to_string())));

        // The stylesheet still answers: the start page is drawn with it.
        assert_eq!(get("/.ts/app.css").status, 200);
    }

    /// The id a remote root is filed under, for the header to show beside a path
    /// that could be on any machine. A local path has none, whatever colons it
    /// happens to contain.
    #[test]
    fn a_remote_root_id_carries_the_bookmark_it_came_from() {
        assert_eq!(super::root_id_bookmark("ssh:iota:/home/hanhua"), Some("iota"));
        assert_eq!(super::root_id_bookmark("s3:bucket:/data"), Some("bucket"));
        assert_eq!(super::root_id_bookmark("/home/x"), None);
        assert_eq!(super::root_id_bookmark(r"C:\Users\x"), None);
        assert_eq!(super::root_id_bookmark("/odd:name/dir"), None);
        // A scheme and nothing after it names nothing.
        assert_eq!(super::root_id_bookmark("ssh::/x"), None);
        assert_eq!(super::root_id_bookmark("ssh:iota"), None);
    }

    #[test]
    fn leaf_of_mirrors_file_name() {
        assert_eq!(leaf_of("/home/x/mix"), Some("mix"));
        assert_eq!(leaf_of("/"), None);
        assert_eq!(leaf_of("name"), Some("name"));
        assert_eq!(leaf_of(""), None);
        assert_eq!(leaf_of("ssh:web:/var/www"), Some("www"));
        // `\` splits only where the host splits on it: a Unix file may carry
        // it in its name, and `Path::file_name` kept it there too.
        #[cfg(not(windows))]
        assert_eq!(leaf_of(r"/x/a\b"), Some(r"a\b"));
        #[cfg(windows)]
        {
            assert_eq!(leaf_of(r"C:\"), None);
            assert_eq!(leaf_of(r"C:\Users\x"), Some("x"));
            assert_eq!(leaf_of(r"\\server\share"), None);
            assert_eq!(leaf_of(r"\\server\share\dir"), Some("dir"));
        }
    }

    /// The scheme grammar, not "all letters": `s3:` is remote, a drive
    /// letter is local, and a path with a stray `:` stays local.
    #[test]
    fn scheme_grammar_decides_remote() {
        assert!(root_id_is_local("/home/x"));
        assert!(root_id_is_local(r"C:\Users\x"));
        assert!(root_id_is_local("/odd:name/dir"));
        assert!(!root_id_is_local("ssh:web:/var/www"));
        assert!(!root_id_is_local("s3:bucket:/data"));
        assert!(!root_id_is_local("ssh2:x:/y"));
    }
}
