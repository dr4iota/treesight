//! Shell around treeserve's router.
//!
//! The same router the CLI serves over HTTP, with no HTTP under it: the window
//! opens on a scheme this shell registers, and every request the webview makes
//! is answered by calling `treeserve::handle`. 303 redirects, Range requests
//! and relative links keep working unchanged, because all of it belongs to the
//! router rather than to the socket that used to carry it. Cookies did not: a
//! custom scheme has no cookie jar, so the shell keeps one itself — see [`Jar`].
//!
//! Nothing is listening on anything. There is no address for another process
//! on the machine to find, which is why there is nothing to authenticate to.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
// Both serve `picker_start_dir`, which is the desktop picker's alone.
#[cfg(desktop)]
use std::sync::mpsc;
use std::thread;
#[cfg(desktop)]
use std::time::Duration;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use treeserve::page::ThemeMode;
use treeserve::{Config, RootStatus};

/// The one window's label — public so a downstream action ([`ShellExt`])
/// can find the same window the shell drives.
pub const WINDOW: &str = "main";
/// How many roots the Recent list keeps.
const RECENT_MAX: usize = 8;

/// Keyboard shortcuts. The window has no menu bar, and on Windows and Linux a
/// menu is what would otherwise carry accelerators, so the shell installs them
/// itself. Clicks need nothing of the sort: the page's controls are ordinary
/// links to `/.ts/…` that `on_navigation` intercepts, which is why this script
/// only ever navigates — the same thing a click does.
mod usage;

/// The two things about a phone's window that CSS cannot ask for itself, set as
/// custom properties for `app.css` to use.
///
/// **How tall the window is.** `100vh` is the window before a soft keyboard, and
/// `100dvh` is the window before this WebView existed on a device that has not
/// heard of `dvh`. `visualViewport.height` is the only one that is the window
/// *now* — keyboard up, address bar gone, both. It goes in as `--vh`, a hundredth
/// of it, which is the shape `calc(var(--vh, 1vh) * 100)` wants and which falls
/// back to exactly `100vh` when nothing set it.
///
/// **Whether `env(safe-area-inset-*)` can be believed.** Android WebViews before
/// Chromium 140 answer zero for all four in a full-screen WebView, and are wrong:
/// there is a status bar up there. For those, a measured floor — the numbers are
/// from a device, they differ between portrait and landscape, and the sides stay
/// zero because which edge a cutout is on is not knowable from here. `app.css`
/// takes `max(env, floor)`, so a WebView that reports properly is never overruled.
///
/// Injected on every navigation ahead of whatever else the shell or an embedder
/// asks for, because a window drawn under the clock is not a preference.
pub const VIEWPORT: &str = r#"
(function () {
 try {
  // A document-start script runs before anything is guaranteed: no root element
  // on a document that has only just committed, no `screen` in some contexts.
  // Nothing here is worth a thrown exception in a script the whole window shares,
  // so the lot is guarded and a failure leaves the CSS on its own defaults.
  var root = document.documentElement;
  if (!root || !root.style) { return; }
  var vv = window.visualViewport;
  var ua = (navigator && navigator.userAgent) || '';
  var chrome = /Chrome\/(\d+)/.exec(ua);
  var legacy = /Android/.test(ua) && (!chrome || +chrome[1] < 140);

  function height() {
    var h = (vv && vv.height) || window.innerHeight || 0;
    // Never zero. Asked early enough, a window has no size yet, and
    // `calc(0px * 100)` is a shell exactly none of the screen tall — which is
    // not a smaller mistake than being too tall, it is an app with nothing in it.
    if (h > 0) { root.style.setProperty('--vh', h * 0.01 + 'px'); }
  }
  function floor() {
    if (!legacy) { return; }
    var o = window.screen && window.screen.orientation;
    var landscape = o ? /^landscape/.test(o.type)
                      : window.matchMedia('(orientation: landscape)').matches;
    root.style.setProperty('--safe-floor-top', landscape ? '24px' : '36px');
    root.style.setProperty('--safe-floor-bottom', '18px');
  }
  // Whether this is a screen people touch. `pointer: coarse` alone answers for
  // the mouse currently attached rather than for the machine: an emulator driven
  // by a host mouse says `fine` and gets a phone build at desktop sizes, and a
  // phone with a Bluetooth mouse would shrink its targets while still being a
  // phone. Either signal is enough, and neither is undone by the other.
  function touch() {
    var coarse = window.matchMedia && window.matchMedia('(pointer: coarse)').matches;
    if (coarse || /Android|iPhone|iPad|iPod/.test(ua)) {
      root.setAttribute('data-touch', '');
    }
  }
  function both() { height(); floor(); }
  // A rotation resolves in stages: the size changes, then the insets, and on a
  // slow device the second one lands a frame or several after the event. Once
  // now and once after it has settled.
  function rotated() { both(); setTimeout(both, 300); }

  touch();
  both();
  // And again once there is a laid-out document to measure, because the first
  // answer was taken before there was one.
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', both);
  }
  addEventListener('load', both);
  addEventListener('resize', both);
  addEventListener('orientationchange', rotated);
  var o = window.screen && window.screen.orientation;
  if (o && o.addEventListener) { o.addEventListener('change', rotated); }
  if (vv) {
    // `resize` is the keyboard; `scroll` is the address bar being dragged away,
    // which fires no resize of the window at all.
    vv.addEventListener('resize', both);
    vv.addEventListener('scroll', height);
  }
 } catch (e) { /* the defaults are a working window; this only ever improves it */ }
})();
"#;

const SHORTCUTS: &str = r#"
addEventListener('keydown', function (e) {
  if (e.altKey && !e.ctrlKey && e.key === 'ArrowLeft') { history.back(); }
  else if (e.altKey && !e.ctrlKey && e.key === 'ArrowRight') { history.forward(); }
  else if (e.ctrlKey && (e.key === 'r' || e.key === 'R')) { location.reload(); }
  else if (e.key === 'F5' && !e.ctrlKey && !e.altKey) { location.reload(); }
  else if (e.ctrlKey && e.key === 'Home') { location.assign('/'); }
  else if (e.ctrlKey && (e.key === 'o' || e.key === 'O')) { location.assign('/.ts/open'); }
  else if (e.ctrlKey && (e.key === 'p' || e.key === 'P')) { location.assign('/.ts/print'); }
  else { return; }
  e.preventDefault();
});
"#;

/// The running server, kept in Tauri's managed state. Public so a downstream
/// action can reach the same server the shell drives —
/// Where the shell's own pages come from.
///
/// A custom scheme, not a port. Windows and Android hand a registered scheme to
/// the webview as `http://<scheme>.localhost`; everywhere else it keeps the
/// scheme it was registered under. Nothing is listening on a socket, so there
/// is no address for another process on the machine to find, and nothing to
/// authenticate to — which is the whole of why the token handshake is gone.
pub const SCHEME: &str = "treesight";

pub fn scheme_base() -> &'static str {
    if cfg!(any(windows, target_os = "android")) {
        "http://treesight.localhost"
    } else {
        "treesight://localhost"
    }
}

/// The forms a page of ours can arrive under, for [`origin_allowed`]. A URL on
/// a non-special scheme has an opaque origin — `treesight://localhost` and
/// `treesight://anything-else` both serialize to "null" — so that half is
/// matched by scheme, which is what the trailing colon means here.
fn shell_origins() -> Vec<String> {
    vec!["treesight:".to_string(), scheme_base().to_string()]
}

/// `app.try_state::<Serving>()` — to re-root it (`Config::set_root_vfs`) and
/// to build URLs on its origin.
pub struct Serving {
    state: Arc<treeserve::State>,
    /// The scheme base every page of ours hangs off.
    origin: String,
    /// The URL the window opens with. Once a plain root: there is no cookie to
    /// collect on the way in any more.
    entry: String,
    /// Whether a page of ours has finished loading in the window yet.
    ///
    /// Re-rooting has to put the new root on screen without leaving the old page
    /// behind in the history — every page this window shows is the same URL, so
    /// a pushed entry is a copy of the one you are on, and Back into it renders
    /// the *new* root under the old page's address. `location.replace` is how a
    /// page is changed without an entry, and it needs a page to run in: before
    /// the window's own first load has committed there is none, and a plain
    /// navigation supersedes that load rather than stacking on it.
    loaded: AtomicBool,
}

impl Serving {
    /// The router's shared state: the `Config` a root opener re-roots and
    /// feeds Places/Recent/status through.
    pub fn state(&self) -> &Arc<treeserve::State> {
        &self.state
    }

    /// The scheme base — for building `/.ts/…` URLs to navigate to.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Where a fresh navigation should start: the served root.
    pub fn entry(&self) -> &str {
        &self.entry
    }
}

/// What a downstream app may hang on the shell. [`run`] is
/// `run_with(generate_context!(), ShellExt::default())`; a downstream build
/// supplies its own context — its own identifier, icons and windows — and its
/// extensions, and everything else here serves both apps from one source.
///
/// Unstable: this is the seam for downstream apps of this repo, not a public
/// API with compatibility promises.
#[derive(Default)]
pub struct ShellExt {
    /// Tried on every navigation before the built-in `/.ts/…` handling.
    /// Returning true claims the URL: the navigation is cancelled and the
    /// action has done whatever it does, exactly like the built-ins.
    pub actions: Vec<Box<dyn Fn(&AppHandle, &tauri::Url) -> bool + Send + Sync>>,
    /// Extra Places entries, appended after the platform's own —
    /// (label, RootId) pairs, like `Config::places`.
    pub extra_places: Vec<Box<dyn Fn(&AppHandle) -> Vec<(String, String)> + Send + Sync>>,
    /// Whole pane sections of the downstream app's own, drawn between Places
    /// and Recent. Computed once, when the server starts; for anything later —
    /// a config UI added a server — call `Config::set_sections` through
    /// [`Serving::state`] and the next page render has it.
    ///
    /// `extra_places` stays for what it is: an entry that belongs *inside* the
    /// platform's Places list rather than in a list of its own.
    #[allow(clippy::type_complexity)]
    pub extra_sections:
        Vec<Box<dyn Fn(&AppHandle) -> Vec<treeserve::PaneSection> + Send + Sync>>,
    /// Whether *this* shell can ask for a folder on a platform where this crate
    /// cannot. `/.ts/open` is a link an action may claim (see [`Self::actions`]),
    /// and a downstream app that claims it knows something this crate does not:
    /// Android has no folder picker, but it does have a folder *grant*, and a
    /// shell that asks for one should have a control to ask from.
    ///
    /// Desktop needs nothing here — the shell's own dialog is the picker.
    pub picker: bool,
    /// What the start page says this program is, in a sentence or two. Plain
    /// text; the page escapes it. A downstream app is a different program with
    /// different reasons to exist — telesight browses machines it has a login
    /// for, which is not something this sentence should have to cover.
    pub intro: Option<String>,
    /// Replaces the shell's keyboard-shortcut script wholesale. A downstream
    /// page can need the very keys the default script binds, so the guard
    /// belongs to whoever knows about that page.
    pub init_script: Option<String>,
    /// Origins the window may navigate to besides the local server. An entry
    /// ending in `:` with no `/` names a scheme (`telesight:`) — custom schemes
    /// have opaque origins, so the scheme is the whole identity. Anything
    /// else must equal the URL's serialized origin exactly
    /// (`http://telesight.localhost`, the form Windows and Android serve custom
    /// protocols on) — equality, not a prefix, so a lookalike host with a
    /// suffix cannot ride the allowlist. Everything not local and not listed
    /// still opens in the user's browser.
    pub allowed_origins: Vec<String>,
    /// Usage pages of the downstream app's own, added to this crate's set and
    /// served from the Usage root. `(path, bytes)` pairs, `/`-joined and
    /// relative; a page whose path is already taken replaces it, which is how
    /// `README.md` comes to name the right program without the rest being copied.
    pub usage_pages: Vec<(&'static str, &'static [u8])>,
    /// Controls of the embedder's own on the header's flag row, drawn with
    /// Refresh and ahead of whatever the page brought. Installed when the server
    /// starts; for anything later — a control that comes and goes — call
    /// `Config::set_flags` through [`Serving::state`].
    ///
    /// A flag that acts on the root should say so with
    /// [`treeserve::HeaderFlag::roots`], or it will be offered against roots it
    /// cannot act on.
    pub flags: Vec<treeserve::HeaderFlag>,
    /// One shot at the builder before the shell finishes it: plugins to
    /// register, mobile-specific setup.
    #[allow(clippy::type_complexity)]
    pub configure:
        Option<Box<dyn FnOnce(tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> + Send>>,
    /// Roots this crate cannot open by itself: a machine with a login, a folder
    /// the platform granted, anything whose RootId is not a path. See
    /// [`RootOpener`] — the choreography around one is this crate's, and only
    /// the opening is the embedder's.
    pub openers: Vec<Arc<dyn RootOpener>>,
}

/// A root that opened: what to serve, what to call it, and the RootId it
/// settled on — which is not always the one that was asked for. `~` becomes a
/// real path once a session can resolve it.
pub struct Opened {
    pub id: String,
    pub vfs: Arc<dyn treeserve::vfs::Vfs>,
    /// A name for the window's title, where the thing has one a person chose.
    /// The header shows the folder and the id; a title has room for a name.
    pub name: Option<String>,
}

/// Opens a kind of root this crate has never heard of.
///
/// Everything around the opening is the same whatever the root is — say what is
/// being opened, get off the thread that answers clicks, serve it, remember it,
/// put the page back if it did not happen — and that part stays here, once,
/// where its several sharp edges are already documented. What an embedder
/// supplies is the middle: given an id, a `Vfs`.
pub trait RootOpener: Send + Sync {
    /// Whether this opener owns the id. Asked in order; the first yes wins.
    fn claims(&self, id: &str) -> bool;

    /// What the wait page calls it while it opens. A name a person would
    /// recognise where there is one, and the id where there is not.
    fn label(&self, app: &AppHandle, id: &str) -> String {
        let _ = app;
        id.to_string()
    }

    /// The opening itself, on a thread of this crate's making — so it may take
    /// as long as a network does, and may put a dialog up and wait for it.
    ///
    /// `None` did not open, and says nothing further: the opener has already
    /// told the reader why, or has decided there is nothing to tell them. A
    /// dismissed password box is the second kind, and the reason this is not a
    /// `Result<_, String>` — an error with a message is not the only way to
    /// fail, and a dialog saying "cancelled" after you cancelled something is
    /// the app arguing with you.
    fn open(&self, app: &AppHandle, id: &str) -> Option<Opened>;

    /// How a root of this kind is doing, for the greying of a row that is only
    /// being *listed*. Called for every remembered id at launch.
    ///
    /// **Must not connect.** Cached status, live session state, a list the
    /// platform already holds — nothing that can block, and nothing that costs
    /// a handshake. Answer [`RootStatus::Ok`] when there is no cheap way to
    /// know: a row that is wrong about being fine costs a click, and a launch
    /// that hangs costs the app.
    fn probe(&self, app: &AppHandle, id: &str) -> RootStatus;
}

/// The extensions after defaults are resolved, in Tauri's managed state so
/// the navigation handler and every `start` can reach them.
struct Ext {
    actions: Vec<Box<dyn Fn(&AppHandle, &tauri::Url) -> bool + Send + Sync>>,
    extra_places: Vec<Box<dyn Fn(&AppHandle) -> Vec<(String, String)> + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    extra_sections:
        Vec<Box<dyn Fn(&AppHandle) -> Vec<treeserve::PaneSection> + Send + Sync>>,
    init_script: String,
    picker: bool,
    intro: Option<String>,
    allowed_origins: Vec<String>,
    usage_pages: Vec<(&'static str, &'static [u8])>,
    openers: Vec<Arc<dyn RootOpener>>,
    flags: Vec<treeserve::HeaderFlag>,
}

struct SharedExt(Arc<Ext>);

pub fn run() {
    run_with(tauri::generate_context!(), ShellExt::default());
}

pub fn run_with(context: tauri::Context<tauri::Wry>, mut ext: ShellExt) {
    let configure = ext.configure.take();
    let ext = Arc::new(Ext {
        actions: ext.actions,
        extra_places: ext.extra_places,
        extra_sections: ext.extra_sections,
        // `init_script` replaces the *shortcuts*. The platform script goes in
        // either way: an embedder swapping key bindings is not asking for a
        // window that draws behind the status bar.
        init_script: format!(
            "{VIEWPORT}\n{}",
            ext.init_script.unwrap_or_else(|| SHORTCUTS.to_string())
        ),
        picker: ext.picker,
        intro: ext.intro,
        allowed_origins: ext.allowed_origins,
        usage_pages: ext.usage_pages,
        openers: ext.openers,
        flags: ext.flags,
    });
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    // Must be registered first. A second launch re-roots the open window when it
    // names a directory (Explorer "open with", drag onto the exe). Desktop only:
    // a phone has no second process to fold in and no argv to read — the system
    // delivers a new intent to the activity that is already running.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(dir) = first_dir_arg(argv.into_iter().skip(1)) {
                open_root(app, dir, true);
            }
            if let Some(w) = app.get_webview_window(WINDOW) {
                let _ = w.set_focus();
            }
        }));
    }
    // The slot the protocol handler serves out of. It has to exist before the
    // builder, which is before there is an AppHandle to build a root from, so
    // `start` fills it in during setup and the handler answers 503 until then.
    let router = Router(Arc::new(OnceLock::new()));
    let jar = Arc::new(Jar::default());
    let opened = Arc::clone(&jar);
    let managed = Arc::clone(&jar);
    let mut builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Router(Arc::clone(&router.0)))
        .manage(JarState(managed))
        .register_asynchronous_uri_scheme_protocol(SCHEME, move |_ctx, request, responder| {
            let slot = Arc::clone(&router.0);
            let jar = Arc::clone(&jar);
            // Off the webview's thread: highlighting a large file is the slow
            // part of answering, and the tiny_http build had a pool of workers
            // for exactly that reason. Doing it here would stop the window
            // painting while it ran.
            thread::spawn(move || responder.respond(serve(&slot, &jar, request)));
        });
    if let Some(f) = configure {
        builder = f(builder);
    }
    builder
        .setup(move |app| {
            app.manage(SharedExt(Arc::clone(&ext)));
            let handle = app.handle().clone();
            // Before the first page is asked for: it is the one that reads them.
            if let Ok(dir) = app.path().app_config_dir() {
                opened.open(dir.join("prefs.txt"));
            }
            // The server and the window go up first, hidden, on a placeholder root
            // that is never seen. Creating the window is the one slow thing left in
            // a cold start — WebView2 spawning its processes, and on a first ever
            // run laying down its user-data folder — and it used to be spent after
            // the folder was settled, with nothing on screen to show for it. Now it
            // overlaps whatever settles the folder, which is either the user
            // reading a dialog or a drive making up its mind. Loading a real page
            // into it is deliberate too: the stylesheet and the layout are warm by
            // the time there is something to paint.
            if let Err(e) = start(&handle) {
                fail(&handle, &e, true);
                return Ok(());
            }
            // A folder is opened when this run was given one, and otherwise not at
            // all: the window comes up on the start page, which says what there is
            // to open. A modal picker in front of a window nobody has seen yet was
            // the first thing the app ever did, and it asked a question the answer
            // to which is often "the one I opened last" — a question the page can
            // put on screen instead of in the way.
            // A phone has no argv to read, and the function that reads one is
            // desktop's — so this is where the two platforms differ, in one line
            // each, rather than in two copies of what follows.
            #[cfg(desktop)]
            let opened = first_dir_arg(std::env::args().skip(1));
            #[cfg(mobile)]
            let opened: Option<PathBuf> = None;
            match opened {
                Some(dir) => open_root(&handle, dir, true),
                // Including on a phone, where there is no argv and no picker to
                // stand in for it: app storage is a Place on that page, one tap
                // away, instead of the root nobody chose.
                None => {
                    if let Some(win) = handle.get_webview_window(WINDOW) {
                        show(&win);
                    }
                }
            }
            Ok(())
        })
        .run(context)
        .expect("error while running treesight");
}

/// First argument that names a readable directory.
#[cfg(desktop)]
fn first_dir_arg<I: Iterator<Item = String>>(args: I) -> Option<PathBuf> {
    args.filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .find_map(|p| p.canonicalize().ok().filter(|p| p.is_dir()))
}

/// Where to open the picker: the newest Recent that names a local path, if it
/// answers straight away.
///
/// A remembered folder can be on a drive that is not there, and handing the
/// picker one of those makes the picker do the waiting we just stopped doing.
/// Half a second on a thread we can walk away from, then the picker decides for
/// itself. The probe thread may sit there for another twenty seconds; it holds
/// nothing but a send end nobody is listening to.
///
/// A remote id is skipped rather than probed: it is not a path this picker —
/// the platform's own, browsing the local filesystem — could start in.
#[cfg(desktop)]
fn picker_start_dir(app: &AppHandle) -> Option<PathBuf> {
    let last = PathBuf::from(
        recent(app)
            .into_iter()
            .find(|id| treeserve::root_id_is_local(id))?,
    );
    let (tx, rx) = mpsc::channel();
    let probe = last.clone();
    thread::spawn(move || {
        let _ = tx.send(probe.is_dir());
    });
    match rx.recv_timeout(Duration::from_millis(500)) {
        Ok(true) => Some(last),
        _ => None,
    }
}

/// Native folder picker. Non-blocking: the blocking variant would deadlock the
/// event loop when called from `setup` or from a navigation handler.
#[cfg(desktop)]
fn ask_for_folder(app: AppHandle, exit_if_cancelled: bool) {
    // The probe inside `picker_start_dir` is on its own thread, but the wait
    // for it was on the caller — a navigation callback, frozen for up to half
    // a second by exactly the case the probe exists for: a dead mapped drive
    // at the head of Recent. So the whole prelude moves off the callback, and
    // only the dialog itself is dispatched back to the thread dialogs belong
    // to.
    thread::spawn(move || {
        let start = picker_start_dir(&app);
        let back = app.clone();
        let _ = app.run_on_main_thread(move || {
            let mut dialog = back.dialog().file().set_title("Choose a folder to browse");
            if let Some(last) = start {
                dialog = dialog.set_directory(last);
            }
            let app = back.clone();
            dialog.pick_folder(move |picked| match picked {
                Some(path) => match path.into_path() {
                    Ok(dir) => open_root(&app, dir, true),
                    Err(e) => {
                        fail(&app, &format!("Cannot use that folder: {e}"), exit_if_cancelled)
                    }
                },
                None if exit_if_cancelled => app.exit(0),
                None => {}
            });
        });
    });
}

/// There is no folder picker here. Choosing a directory on a phone means a
/// per-directory grant from the system picker, and this shell does not ask for
/// one yet — so say so, in the same place the desktop would have put a dialog.
///
/// `exit_if_cancelled` keeps the desktop signature so no caller has to know
/// which platform it is on; here it means "there is no page to say this over",
/// and the answer is the app's own storage rather than `exit(0)`. A window that
/// closes itself reads as a crash on a phone.
#[cfg(mobile)]
fn ask_for_folder(app: AppHandle, exit_if_cancelled: bool) {
    if !exit_if_cancelled {
        fail(&app, "Choosing a folder is not available on this device yet.", false);
        return;
    }
    match app_storage_dir(&app) {
        Some(dir) => open_root(&app, dir, true),
        None => fail(&app, "This device gave the app no storage.", true),
    }
}

/// The app's own directory: private, always there, and granted by nobody. No
/// permission prompt, no store review, and it survives everything except an
/// uninstall. Created on first use, because a root that does not exist cannot
/// be served and an empty one is the honest starting state.
///
/// A folder of the reader's own, and not the directory above it. On Android
/// `app_data_dir` and `app_config_dir` are the same path — the app's private
/// root — so serving that shows them `cache`, `code_cache`, `shared_prefs` and
/// every file the shell or its embedder keeps, which for one of them is a
/// private key and a file with passwords in it.
///
/// `user_files` rather than the framework's own `files`: that one is
/// `getFilesDir()`, which any library in the process may decide to write to, and
/// this is a folder whose whole promise is that what is in it was put there by
/// the person looking at it. The spelling matches the neighbours — `code_cache`,
/// `shared_prefs`, `no_backup` — because it sits among them.
#[cfg(mobile)]
fn app_storage_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("user_files");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Serves `dir`, resolving it off the UI thread.
///
/// `canonicalize` is a syscall with no time limit. A drive letter mapped to a host
/// that is switched off takes as long as the redirector takes to give up — twenty
/// seconds, measured — and this used to run inside `on_navigation`, which is the
/// main thread: the whole window froze, including the part of it that would have
/// said why. So the waiting happens on a thread, the window says what it is
/// opening while that goes on, and it comes back either way.
///
/// `remember` keeps it out of the Recent list, which is what the pane's Places
/// need: that list is fixed, and a Place that added itself to Recent would just
/// be duplicating a shortcut the pane already shows.
fn open_root(app: &AppHandle, dir: PathBuf, remember: bool) {
    let app = app.clone();
    thread::spawn(move || {
        // The wait page is drawn from here too, and not from the callback: it
        // reads the window's URL to have something to go back to, and on Android
        // that read waits on the thread the callback is holding.
        let previous = show_opening(&app, &dir);
        let resolved = match dir.canonicalize() {
            Ok(d) if d.is_dir() => Ok(d),
            // There, and not a folder: as good as gone for our purposes.
            Ok(_) => Err(RootStatus::Missing),
            Err(e) => Err(classify(&e)),
        };
        let back = app.clone();
        // Windows and server state are the main thread's to touch.
        let _ = app.run_on_main_thread(move || match resolved {
            Ok(dir) => serve_root(&back, dir, remember),
            Err(status) => {
                // The pane said what it knew when it was drawn; this is fresher,
                // so record it before going back to a page that will show it.
                if let Some(serving) = back.try_state::<Serving>() {
                    serving
                        .state()
                        .cfg
                        .set_root_status(treeserve::util::display_path(&dir), status);
                }
                fail(&back, &cannot_open(&dir, status), false);
                open_failed(&back, previous);
            }
        });
    });
}

/// The first opener that owns this id, if any. Asked in the order the embedder
/// listed them.
fn opener_for(app: &AppHandle, id: &str) -> Option<Arc<dyn RootOpener>> {
    let ext = app.try_state::<SharedExt>()?;
    ext.0
        .openers
        .iter()
        .find(|o| o.claims(id))
        .map(Arc::clone)
}

/// Serves a root an embedder opened. The same choreography `open_root` gives a
/// folder, around an opening this crate could not have done.
///
/// `remember` for the same reason it is there: a Place is already in the pane
/// and would only be duplicating itself in Recent.
fn open_by(app: &AppHandle, opener: Arc<dyn RootOpener>, id: String, remember: bool) {
    let app = app.clone();
    thread::spawn(move || {
        // From this thread and not from the callback that asked, for the reason
        // written on `show_opening`: the wait page reads the window's URL, and
        // on Android that read waits on the thread the callback is holding. The
        // opener is about to do something slow on this thread, which is the
        // other half of why it is here.
        let previous = show_waiting(&app, &opener.label(&app, &id));
        let opened = opener.open(&app, &id);
        let back = app.clone();
        // Windows and server state are the main thread's to touch.
        let _ = app.run_on_main_thread(move || match opened {
            Some(opened) => serve_opened(&back, opened, remember),
            // The opener has said whatever needed saying, including nothing.
            // All that is left is the page that was on screen before.
            None => open_failed(&back, previous),
        });
    });
}

/// Points the window at a root that is already open. Main thread only.
fn serve_opened(app: &AppHandle, opened: Opened, remember: bool) {
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    serving.state().cfg.set_root_vfs(treeserve::Root {
        id: opened.id.clone(),
        vfs: opened.vfs,
    });
    serving.state().cfg.set_root_name(opened.name);
    if remember {
        // Off this thread: recording Recent reads and writes the config
        // directory, and asking where that is is a platform call on Android —
        // dispatched, with no timeout, to the very thread this closure runs
        // on. The same rule that put `pin_root_id` behind `off_the_callback`;
        // the pane simply shows the new row one render later.
        let id = opened.id.clone();
        off_the_callback(app, move |app| remember_root_id(app, &id));
    }
    replace_page(app, serving.entry());
    if let Some(win) = app.get_webview_window(WINDOW) {
        show(&win);
    }
}

/// Why a folder did not open, in the words the pane uses for the same thing — so
/// that a drive which the list calls "not available" is not called "missing" here.
fn cannot_open(dir: &Path, status: RootStatus) -> String {
    let path = treeserve::util::display_path(dir);
    match status {
        RootStatus::Unreachable => {
            format!("{path} is not available.\n\nThe drive or share did not answer.")
        }
        _ => format!("{path} is no longer there."),
    }
}

/// Points the window at a folder that has been resolved. Main thread only.
fn serve_root(app: &AppHandle, dir: PathBuf, remember: bool) {
    if remember {
        // Off this thread — see `serve_opened`: Recent lives in the config
        // directory, and locating that is a platform call on Android.
        let id = treeserve::util::display_path(&dir);
        off_the_callback(app, move |app| remember_root_id(app, &id));
    }

    if let Some(serving) = app.try_state::<Serving>() {
        let id = treeserve::util::display_path(&dir);
        serving.state().cfg.set_root(dir.clone());
        // A Place has a name and the folder it points at usually does not — "App
        // storage" against a path ending in the application id, which is the
        // program telling the reader its own package name. `set_root` clears the
        // name, so this goes after it, and only for a path the fixed list knows:
        // a folder opened from the picker or from Recent is named after itself,
        // which is all anybody could call it.
        if let Some((label, _)) = serving.state().cfg.places.iter().find(|(_, p)| *p == id) {
            serving.state().cfg.set_root_name(Some(label.clone()));
        }
        // Re-show: the root the window was showing has just been replaced.
        // `entry` is the served root, which is all it is now that there is no
        // cookie to collect on the way in. The page-load hook retitles.
        //
        // Replaced rather than navigated to, because a navigation would push:
        // every page of this window wears the same address, so the entry left
        // behind is a copy of the page you are on — Back into it re-renders
        // whatever root is current, and on the first folder of a run it made a
        // live Back button out of a window with nothing behind it at all.
        replace_page(app, &serving.entry);
        if let Some(win) = app.get_webview_window(WINDOW) {
            // It may still be hidden: the window is built while the picker is up,
            // and this is the first moment there is a folder to put in it.
            // Idempotent for every later re-root.
            show(&win);
        }
        return;
    }

    // The server is up from the moment the app is, so re-rooting is the only
    // thing that ever happens here; there is no first-folder special case left.
    match start(app) {
        Ok(()) => {
            if let Some(win) = app.get_webview_window(WINDOW) {
                show(&win);
            }
        }
        Err(e) => fail(app, &e, true),
    }
}

/// Says what is being opened, for as long as opening it takes, and hands back the
/// page it replaced so a failed open can put it there again.
///
/// Only with a window already on screen. Before that there is nothing to put it
/// in, and a folder named on the command line is not something anybody is sitting
/// there watching fail.
///
/// Desktop only, that last part: on Android `is_visible` is a stub that warns and
/// answers false whatever the window is doing, which read as "never show the wait
/// page" — and there the window cannot be hidden while a row can be tapped.
fn show_opening(app: &AppHandle, dir: &Path) -> Option<tauri::Url> {
    show_waiting(app, &treeserve::util::display_path(dir))
}

/// The same, for a root that is not a path and has a name of its own.
fn show_waiting(app: &AppHandle, label: &str) -> Option<tauri::Url> {
    let serving = app.try_state::<Serving>()?;
    let win = app.get_webview_window(WINDOW)?;
    if cfg!(desktop) && !win.is_visible().unwrap_or(false) {
        return None;
    }
    let previous = win.url().ok();
    let url = format!(
        "{}/.ts/wait?path={}",
        serving.origin,
        treeserve::util::percent_encode(label)
    );
    replace_page(app, &url);
    previous
}

/// After an open that did not happen: back to the exact page that was on screen,
/// rather than to the served root — nothing was re-rooted, so nobody should lose
/// their place over it. With nothing on screen to go back to — a bad path on the
/// command line — ask for a folder instead of leaving a window that never appears.
fn open_failed(app: &AppHandle, previous: Option<tauri::Url>) {
    match (app.get_webview_window(WINDOW), previous) {
        (Some(_), Some(url)) => replace_page(app, url.as_str()),
        _ => ask_for_folder(app.clone(), true),
    }
}

/// The router's state, once there is one. Shared between the protocol handler
/// registered on the builder and the `start` that eventually fills it.
struct Router(Arc<OnceLock<Arc<treeserve::State>>>);

/// The same [`Jar`] the protocol handler writes, where `shell_action` can reach
/// it: the toggles are answered there, not by a page load.
struct JarState(Arc<Jar>);

/// Cookies, for a scheme that has none.
///
/// Preferences are cookies — `/.ts/set` answers with `Set-Cookie: ts_theme=…`
/// and every page after reads them back off the request. That is a *network*
/// mechanism, and a custom scheme never goes near the network: on Linux and
/// macOS these pages come from `treesight://localhost`, whose origin serializes
/// to "null", so WebKit stores nothing and sends nothing back. Both toggles
/// bounced through `/.ts/set` and returned to the same page unchanged.
///
/// So the shell holds the jar. It keeps what the router asked to store and folds
/// it into the `Cookie` on every request going the other way — see [`with_jar`]
/// for which side wins where a platform has a working store of its own, and
/// `set_pref` for the 303 that a custom scheme cannot carry out either.
///
/// `Path` and `Max-Age` are dropped on the floor: every page is under `/`, and
/// the file this is written to *is* the expiry. Nothing here is a secret; the
/// whole content is which theme you picked.
#[derive(Default)]
struct Jar {
    cookies: Mutex<BTreeMap<String, String>>,
    /// Where the jar lives. Set in `setup`, because the protocol handler is
    /// registered on the builder — before there is an `AppHandle` to ask.
    file: OnceLock<PathBuf>,
}

impl Jar {
    /// Reads whatever a previous run stored. A theme chosen once is chosen.
    fn open(&self, file: PathBuf) {
        if let Ok(text) = fs::read_to_string(&file) {
            *self.cookies.lock().unwrap() = jar_from_text(&text);
        }
        let _ = self.file.set(file);
    }

    /// Remembers what a reply asked to store, leaving the header in place for a
    /// platform that can honour it.
    fn store(&self, reply: &treeserve::Reply) {
        self.put(
            reply
                .headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("Set-Cookie"))
                .filter_map(|(_, v)| cookie_pair(v))
                .collect(),
        );
    }

    /// One preference, chosen somewhere other than a reply — see [`set_theme`].
    fn set(&self, name: &str, value: &str) {
        self.put(vec![(name.to_string(), value.to_string())]);
    }

    fn put(&self, pairs: Vec<(String, String)>) {
        if pairs.is_empty() {
            return;
        }
        let mut cookies = self.cookies.lock().unwrap();
        cookies.extend(pairs);
        if let Some(file) = self.file.get() {
            if let Some(dir) = file.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(file, jar_text(&cookies));
        }
    }
}

/// The name and value out of a `Set-Cookie` value: everything up to the first
/// `;` is the pair, the attributes after it are none of this jar's business.
fn cookie_pair(value: &str) -> Option<(String, String)> {
    let (k, v) = value.split(';').next()?.split_once('=')?;
    let (k, v) = (k.trim(), v.trim());
    (!k.is_empty()).then(|| (k.to_string(), v.to_string()))
}

/// One `name=value` per line, in the shape `recent.txt` set: a file somebody can
/// read, and delete to go back to the defaults.
fn jar_text(cookies: &BTreeMap<String, String>) -> String {
    cookies.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
}

fn jar_from_text(text: &str) -> BTreeMap<String, String> {
    text.lines().filter_map(|l| cookie_pair(l.trim())).collect()
}

/// Answers one webview request out of the router, or says there is nothing to
/// answer with yet.
fn serve(
    slot: &OnceLock<Arc<treeserve::State>>,
    jar: &Jar,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let Some(state) = slot.get() else {
        return reply_to_response(treeserve::Reply {
            status: 503,
            headers: vec![("Content-Type".into(), "text/plain; charset=utf-8".into())],
            body: treeserve::Body::Text("not started".into()),
        });
    };
    let reply = treeserve::handle(state, &to_req(&request, jar));
    jar.store(&reply);
    reply_to_response(reply)
}

/// A webview request, in the terms the router speaks. `Req::url` is path and
/// query only, which is what tiny_http's `url()` gave it and what every route
/// here matches on — the scheme and host are this shell's own and say nothing.
fn to_req(request: &tauri::http::Request<Vec<u8>>, jar: &Jar) -> treeserve::Req {
    let uri = request.uri();
    let headers = with_jar(
        request
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_string(), v.to_string())))
            .collect(),
        jar,
    );
    treeserve::Req {
        url: match uri.query() {
            Some(q) => format!("{}?{}", uri.path(), q),
            None => uri.path().to_string(),
        },
        headers,
        is_get: request.method() == tauri::http::Method::GET,
    }
}

/// The request's headers with the jar's cookies folded into whatever the webview
/// brought.
///
/// The jar wins where both have an answer. `/.ts/set` is answered by
/// `shell_action` and never reaches the protocol handler, so nothing writes to
/// the webview's own store any more — a `Cookie` from it is a leftover from
/// before this shell kept its own, which on Windows (a real origin, a real jar)
/// would otherwise out-vote the toggle just clicked. Anything it sends that the
/// jar has no opinion on is left alone.
fn with_jar(headers: Vec<(String, String)>, jar: &Jar) -> Vec<(String, String)> {
    let (cookie, mut rest): (Vec<_>, Vec<_>) = headers
        .into_iter()
        .partition(|(k, _)| k.eq_ignore_ascii_case("Cookie"));
    let mut cookies: BTreeMap<String, String> = cookie
        .iter()
        .flat_map(|(_, v)| v.split(';'))
        .filter_map(|p| cookie_pair(p.trim()))
        .collect();
    cookies.extend(jar.cookies.lock().unwrap().clone());
    if !cookies.is_empty() {
        rest.push(("Cookie".to_string(), cookie_list(&cookies)));
    }
    rest
}

/// `a=1; b=2` — a `Cookie` header's value.
fn cookie_list(cookies: &BTreeMap<String, String>) -> String {
    cookies
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// A decided reply, as bytes the webview will take.
///
/// The stream is read out here rather than handed over: a custom protocol
/// answers with a body, not with a handle to pull on. It costs nothing on a
/// page and little on a picture, and the case that would hurt — a film — never
/// arrives whole, because the webview asks for it a range at a time and each
/// range is its own small answer.
fn reply_to_response(reply: treeserve::Reply) -> tauri::http::Response<Vec<u8>> {
    let mut builder = tauri::http::Response::builder().status(reply.status);
    for (k, v) in &reply.headers {
        builder = builder.header(k, v);
    }
    let body = match reply.body {
        treeserve::Body::Empty => Vec::new(),
        treeserve::Body::Text(text) => text.into_bytes(),
        treeserve::Body::Stream { mut reader, len } => {
            let mut buf = Vec::with_capacity(len as usize);
            match reader.read_to_end(&mut buf) {
                Ok(_) => buf,
                Err(e) => {
                    return tauri::http::Response::builder()
                        .status(500)
                        .header("Content-Type", "text/plain; charset=utf-8")
                        .body(format!("read failed: {e}").into_bytes())
                        .expect("500 is a response");
                }
            }
        }
    };
    builder.body(body).expect("reply is a response")
}

fn show(win: &tauri::WebviewWindow) {
    let _ = win.show();
    let _ = win.set_focus();
}

/// Brings up the server and the window, with nothing open.
///
/// Whoever has a folder opens it after this — from argv, from the picker, from a
/// Place. The window is built either way, because the page that says what there
/// is to open is a page like any other.
fn start(app: &AppHandle) -> Result<(), String> {
    let ext = Arc::clone(&app.state::<SharedExt>().0);
    let mut cfg = Config::rootless();
    // Turns on the page's own chooser: path bar, history buttons, Places,
    // Recent and the picker button. Only ever set here — a server reachable by
    // anything but this window has no business offering them.
    cfg.app_ui = true;
    // And whether that chooser can ask this platform for a folder at all: our own
    // dialog on a desktop, or a downstream shell that claims `/.ts/open` and has
    // something to ask with where we have not.
    cfg.picker = cfg!(desktop) || ext.picker;
    // The name on the start page and in the window title, which have no folder
    // to be named after. From Tauri's own product name rather than this crate's:
    // a downstream shell embedding this one is a different program, and it was
    // calling itself treesight on both.
    cfg.app_name = Some(
        app.config()
            .product_name
            .clone()
            .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string()),
    );
    cfg.app_version = Some(app.config().version.clone().unwrap_or_else(|| {
        env!("CARGO_PKG_VERSION").to_string()
    }));
    cfg.intro = ext.intro.clone();
    cfg.places = places(app)
        .into_iter()
        .map(|(label, dir)| (label, treeserve::util::display_path(&dir)))
        .chain(ext.extra_places.iter().flat_map(|f| f(app)))
        // Last, because it is the one row that is not a folder on this machine —
        // and the one row that is there on a device where nothing else is yet.
        .chain(std::iter::once((
            usage::LABEL.to_string(),
            usage::ROOT_ID.to_string(),
        )))
        .collect();
    cfg.set_recent(recent(app));
    cfg.set_pinned(pinned(app));
    cfg.set_sections(ext.extra_sections.iter().flat_map(|f| f(app)).collect());
    cfg.set_flags(ext.flags.iter().map(clone_flag).collect());

    let state = treeserve::state_for(cfg);
    // The protocol handler was registered before this ran and has been waiting
    // for something to serve; handing it the state is what opens the shop.
    if app.state::<Router>().0.set(Arc::clone(&state)).is_err() {
        return Err("the router was already started".to_string());
    }
    let origin = scheme_base().to_string();
    let entry = format!("{origin}/");
    // Before the state is handed over: the window is built after that, and this is
    // the last moment its title can be read from the config that carries it.
    let title = window_title(&state.cfg);
    app.manage(Serving {
        state,
        origin: origin.clone(),
        entry: entry.clone(),
        loaded: AtomicBool::new(false),
    });

    let win = WebviewWindowBuilder::new(
        app,
        WINDOW,
        WebviewUrl::CustomProtocol(entry.parse().map_err(|e| format!("bad url: {e}"))?),
    )
    .title(title)
    .inner_size(1200.0, 850.0)
    .min_inner_size(480.0, 360.0)
    // Shown by whoever knows there is something worth showing — `open_root`,
    // once it has a folder. A window built before the picker has been answered
    // would otherwise flash a placeholder.
    .visible(false)
    .initialization_script(ext.init_script.as_str())
    // The title follows the served root, which "Open Folder…" can change, so
    // it is refreshed on every page load rather than only at window creation.
    .on_page_load({
        let app = app.clone();
        let shell = shell_origins();
        move |win, payload| {
            // Only for a page this server answered. An embedder's own page — a
            // terminal, a config editor — carries its own title, and retitling
            // the window from the served root left it named after a folder that
            // page has nothing to do with.
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && origin_allowed(&shell, payload.url())
                && let Some(serving) = app.try_state::<Serving>()
            {
                // There is a page here now, and one that can be told to replace
                // itself — which is how a re-root avoids leaving a copy of the
                // page behind it in the history. See `Serving::loaded`.
                serving.loaded.store(true, Ordering::Relaxed);
                let _ = win.set_title(&window_title(&serving.state().cfg));
            }
        }
    })
    .on_navigation({
        let app = app.clone();
        let ext = Arc::clone(&ext);
        let shell = shell_origins();
        move |url| {
            // Downstream actions first: a URL an extension claims is handled
            // entirely by it, the way `/.ts/open` is handled below.
            if ext.actions.iter().any(|a| a(&app, url)) {
                return false;
            }
            // By scheme where the origin is opaque, by whole origin where it is
            // not — never by prefix. `http://treesight.localhost.evil.com`
            // starts with this origin's text and is someone else's site.
            if origin_allowed(&shell, url) {
                // The page's own chrome. These paths are not routes: the server
                // never re-roots itself and would answer 404, so the whole
                // capability lives here, in the one client that may have it.
                if shell_action(&app, url) {
                    return false;
                }
                // A "Download" link: ask where to put it and copy from disk,
                // rather than leaving it to a webview download stack that is
                // invisible on some platforms and absent on others.
                if is_download_link(url) && save_as(&app, url) {
                    return false;
                }
                return true;
            }
            // Origins an extension serves itself — a plugin scheme page.
            if origin_allowed(&ext.allowed_origins, url) {
                return true;
            }
            // Links out of the served tree belong in the user's browser. From a
            // thread, because on mobile the opener is a plugin round trip with
            // no timeout on it, dispatched to the very thread this callback runs
            // on — a tapped link in a rendered README hung the app for good.
            // The answer was already thrown away, so nothing waits for this.
            let app = app.clone();
            let url = url.clone();
            thread::spawn(move || {
                let _ = app.opener().open_url(url.as_str(), None::<&str>);
            });
            false
        }
    })
    // A link that asks for a window of its own: `target="_blank"`, or
    // `window.open`. The navigation handler above never sees one — a webview
    // offers it as a new-window request instead — so with nothing listening
    // here the click did nothing at all, which is what an external link in a
    // rendered document written as raw HTML used to do. This app is one window
    // by design, so the answer is always `Deny`, and then the same two answers
    // the navigation handler gives: in the tree, load it here; out of it, hand
    // it to the browser.
    //
    // Desktop only in effect. Android's WebView is left with multiple windows
    // unsupported, which makes it route `target="_blank"` through the
    // navigation callback like any other link, and it is already answered
    // there.
    .on_new_window({
        let app = app.clone();
        let ext = Arc::clone(&ext);
        let shell = shell_origins();
        move |url, _features| {
            match origin_allowed(&shell, &url) || origin_allowed(&ext.allowed_origins, &url) {
                // Navigated to and not replaced, unlike everything else this
                // shell puts on screen: a link somebody followed is a step
                // forward, and the only reason it came through here rather than
                // through the navigation handler is the window it asked for.
                true => {
                    if let Some(win) = app.get_webview_window(WINDOW) {
                        let _ = win.navigate(url);
                    }
                }
                false => {
                    let app = app.clone();
                    thread::spawn(move || {
                        let _ = app.opener().open_url(url.as_str(), None::<&str>);
                    });
                }
            }
            tauri::webview::NewWindowResponse::Deny
        }
    })
    // Backstop for downloads the webview starts by itself — a PDF WKWebView
    // declines to render, say. Without a destination those fail silently.
    .on_download({
        let app = app.clone();
        // Resolved once, on a thread, before any download asks. The callback
        // has to answer with a destination before it returns, so it cannot go
        // looking — and on Android asking where Downloads is is a platform
        // call with no timeout, dispatched to the very thread webview
        // callbacks run on. A download that races the one resolution simply
        // gets no destination, which is what a missing Downloads dir already
        // meant.
        let downloads: Arc<std::sync::OnceLock<PathBuf>> = Arc::new(std::sync::OnceLock::new());
        {
            let app = app.clone();
            let downloads = Arc::clone(&downloads);
            thread::spawn(move || {
                if let Ok(dir) = app.path().download_dir() {
                    let _ = downloads.set(dir);
                }
            });
        }
        move |_webview, event| {
            match event {
                tauri::webview::DownloadEvent::Requested { url, destination } => {
                    if let Some(dir) = downloads.get() {
                        *destination = dir.join(download_name(&url));
                    }
                }
                tauri::webview::DownloadEvent::Finished { path, success, .. } => match (success, path)
                {
                    (true, Some(p)) => notify(&app, &format!("Saved to {}", p.display())),
                    (false, _) => fail(&app, "The download did not finish.", false),
                    _ => {}
                },
                _ => {}
            }
            true
        }
    })
    .build()
    .map_err(|e| format!("Cannot create the window: {e}"))?;

    check_roots(app);

    // Dropping a folder on the window re-roots; dropping a file opens its page.
    win.on_window_event({
        let app = app.clone();
        move |event| {
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
                && let Some(dir) = paths.iter().find(|p| p.is_dir())
            {
                open_root(&app, dir.clone(), true);
            }
        }
    });

    Ok(())
}

/// Finds out what the pane's shortcuts actually are, off the critical path.
///
/// The two lists go out unchecked — that is what makes them free — and this
/// catches up with them. One thread per path, because the entire problem is that
/// a path can take twenty seconds to answer and the other dozen should not be
/// queued behind it. Each answer is recorded as it lands, so any page rendered
/// after it greys that entry out and says why.
///
/// Recent then prunes itself: an entry that is not there is dropped from the
/// file, so the next launch does not carry it. Once, from here, when every check
/// is in — one writer, no lost updates — and only from the file. The list on
/// screen keeps it, greyed. An entry vanishing from under the pointer is worse
/// than one that says what is wrong with it.
fn check_roots(app: &AppHandle) {
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    let state = Arc::clone(serving.state());
    let file = recent_file(app);
    let app = app.clone();
    thread::spawn(move || {
        let recent = state.cfg.recent();
        let pinned = state.cfg.pinned();
        let all: Vec<String> = state
            .cfg
            .places
            .iter()
            .map(|(_, id)| id.clone())
            .chain(pinned.iter().map(|p| p.id.clone()))
            .chain(recent.iter().cloned())
            .collect();
        // A root that is not a path is asked of whoever owns that kind, and only
        // ever for what it already knows: `probe` is forbidden from connecting,
        // because this loop exists precisely because a probe can hang. An id
        // nobody claims is left alone rather than called missing — the embedder
        // that wrote it may simply not be listening yet.
        for id in all.iter().filter(|id| !treeserve::root_id_is_local(id)) {
            if let Some(opener) = opener_for(&app, id) {
                state.cfg.set_root_status(id.clone(), opener.probe(&app, id));
            }
        }
        let ids: Vec<String> = all
            .into_iter()
            .filter(|id| treeserve::root_id_is_local(id))
            .collect();

        let checks: Vec<_> = ids
            .into_iter()
            .map(|id| {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let status = match fs::metadata(Path::new(&id)) {
                        Ok(m) if m.is_dir() => RootStatus::Ok,
                        // Something is there, but not a folder any more.
                        Ok(_) => RootStatus::Missing,
                        Err(e) => classify(&e),
                    };
                    state.cfg.set_root_status(id.clone(), status);
                    (id, status)
                })
            })
            .collect();

        let answers: Vec<(String, RootStatus)> =
            checks.into_iter().filter_map(|h| h.join().ok()).collect();

        // Recent only. A pinned row is one the reader put there, and a folder on a
        // drive that is not plugged in today is not a mistake to tidy up; it greys
        // and says why, and stays until they say otherwise.
        let gone: Vec<String> = answers
            .iter()
            .filter(|(id, status)| *status != RootStatus::Ok && recent.contains(id))
            .map(|(id, _)| id.clone())
            .collect();
        if let (Some(file), false) = (file, gone.is_empty()) {
            prune_recent(&file, &gone);
        }

        // The page on screen went out before these answers arrived and cannot show
        // them: it is static, and there is no script in it to change its mind. A
        // fast answer beats the first render anyway — an empty DVD drive says
        // "not ready" at once — but the twenty-second ones land long after, which
        // looked like the checks not happening at all. So the shell asks the window
        // to load itself again, which re-renders the pane; the page stays as static
        // as it was, and the one asking is the shell, as it is for the keyboard
        // shortcuts. Once, and only if an answer changed anything, since a reload
        // costs the reader their scroll position. Not while something has focus,
        // which would cost them what they had typed into it.
        if answers.iter().any(|(_, status)| *status != RootStatus::Ok) {
            let back = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(win) = back.get_webview_window(WINDOW) {
                    let _ = win.eval(
                        "var a = document.activeElement; \
                         if (!a || (a.tagName !== 'INPUT' && a.tagName !== 'TEXTAREA')) \
                         location.reload();",
                    );
                }
            });
        }
    });
}

/// What an error from looking at a path means for the pane, and for what we tell
/// anyone who clicked it.
///
/// `ErrorKind` alone gets this wrong, and did: a mapped drive whose host is off
/// answers `ERROR_BAD_NETPATH`, which std folds into `NotFound` — the same kind a
/// deleted folder gives — so `Z:` reported itself as *missing*, which says the
/// share is empty when what happened is that nobody answered. The codes that mean
/// "ask again later" are named here instead, and the kind is only the fallback.
fn classify(e: &io::Error) -> RootStatus {
    if unreachable_code(e) {
        return RootStatus::Unreachable;
    }
    match e.kind() {
        io::ErrorKind::NotFound => RootStatus::Missing,
        // A drive that is not ready, a folder we may not look into: something is
        // there, we just cannot see it. Not the same as gone.
        _ => RootStatus::Unreachable,
    }
}

/// ERROR_NOT_READY, ERROR_BAD_NETPATH, ERROR_DEV_NOT_EXIST, ERROR_UNEXP_NET_ERR,
/// ERROR_NETNAME_DELETED, ERROR_BAD_NET_NAME, ERROR_NO_NET_OR_BAD_PATH,
/// ERROR_NETWORK_UNREACHABLE.
#[cfg(windows)]
fn unreachable_code(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(21 | 53 | 55 | 59 | 64 | 67 | 1222 | 1231)
    )
}

/// ENODEV, ENETDOWN, ENETUNREACH, ETIMEDOUT, EHOSTDOWN, EHOSTUNREACH, ESTALE —
/// the same answer from an NFS or SMB mount whose server has gone.
#[cfg(not(windows))]
fn unreachable_code(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(19 | 100 | 101 | 110 | 112 | 113 | 116))
}

/// Drops paths from the Recent file, leaving everything else as it is — the file
/// may have been rewritten by `remember_root` while the checks were running.
fn prune_recent(file: &Path, gone: &[String]) {
    let Ok(text) = fs::read_to_string(file) else {
        return;
    };
    // Lines written before the RootId change hold the verbatim form
    // (`\\?\C:\…`), so each line is normalized the same way the ids were
    // before comparing — otherwise a dead pre-upgrade entry never leaves.
    let kept: String = text
        .lines()
        .map(str::trim)
        .filter(|l| {
            let norm = treeserve::util::display_path(Path::new(l));
            !l.is_empty() && !gone.iter().any(|g| *g == norm)
        })
        .map(|l| format!("{l}\n"))
        .collect();
    let _ = fs::write(file, kept);
}

/// `?dl=1`, the query the server's "Download" links carry.
fn is_download_link(url: &tauri::Url) -> bool {
    url.query_pairs().any(|(k, v)| k == "dl" && v == "1")
}

/// Last path segment of a URL, for naming a saved file.
fn download_name(url: &tauri::Url) -> String {
    url.path_segments()
        .and_then(|mut s| s.next_back().filter(|s| !s.is_empty()))
        .map(percent_decode)
        .unwrap_or_else(|| "download".to_string())
}

/// Claims a download link, and answers it on a thread.
///
/// Everything the answer needs can wait: resolving the URL and asking for its
/// metadata are network round trips on a remote root, and finding the downloads
/// folder is a platform call with no timeout on it on mobile. This is called
/// from `on_navigation` — the thread the window answers clicks on, and on
/// Android the one every platform call is dispatched to, where waiting for one
/// is waiting for yourself.
///
/// So the link is always claimed. It is only ever drawn for a file; one made by
/// hand that names something else says so from the thread rather than falling
/// through to a navigation, which would arrive back here.
fn save_as(app: &AppHandle, url: &tauri::Url) -> bool {
    // The router's own gate, mirrored — where the backend has no copy to give,
    // the link is not claimed at all, and the navigation proceeds to the
    // server, whose `?dl=1` handling turns it back into the ordinary view.
    // Claiming first made that gate unreachable: any link in served content
    // could write `?dl=1` and start a copy the button would never have offered.
    let downloadable = app
        .try_state::<Serving>()
        .and_then(|serving| serving.state().cfg.root())
        .is_some_and(|root| root.vfs.downloadable());
    if !downloadable {
        return false;
    }
    let app = app.clone();
    let url = url.clone();
    thread::spawn(move || save_asked(&app, &url));
    true
}

/// The other half of [`save_as`], off the callback.
///
/// The URL is resolved back to its path with the server's own checks, so the
/// bytes are copied straight from the served tree — no second HTTP round trip,
/// and nothing outside the root can be reached.
fn save_asked(app: &AppHandle, url: &tauri::Url) {
    // No window state at all: nothing was clicked in a served page, so there is
    // nobody to tell.
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    let Some(root) = serving.state().cfg.root() else {
        return;
    };
    let Ok(target) = treeserve::resolve_in_root(root.vfs.as_ref(), url.path()) else {
        say(app, &format!("{} is not in this folder.", url.path()));
        return;
    };
    if !root.vfs.metadata(&target.path).map(|m| m.is_file).unwrap_or(false) {
        say(app, &format!("{} is not a file to save.", url.path()));
        return;
    }
    // The save sheet on a phone hands back a `content://` URI, and the copy
    // below wants a path: `into_path` fails on one, which this code reads as a
    // cancelled dialog — so the user chose a destination and nothing was
    // written and nothing was said. Say it before the sheet instead. Writing
    // through the URI means the dialog plugin's descriptor or the fs plugin,
    // and neither is wired here yet; docs/todo.md carries it.
    // On a phone there is no dialog to ask where: the save sheet hands back a
    // `content://` destination this cannot write to, and the reader has no
    // filesystem to point at anyway. So the copy goes to the one folder that is
    // theirs and browsable — Files, the Place at the top of the pane — under the
    // file's own name, and the app says where it went. Only a root whose
    // backend answers `Vfs::downloadable` gets here — `save_as` checks it
    // before claiming the link, the same answer that decides whether the
    // button is drawn at all.
    #[cfg(mobile)]
    {
        save_into_files(app, &target, &root);
        return;
    }

    #[cfg(desktop)]
    {
    let app = app.clone();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Save file")
        .set_file_name(target.rel.last().cloned().unwrap_or_default());
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    let vfs = Arc::clone(&root.vfs);
    let src = target.path;
    dialog.save_file(move |dest| {
        let Some(dest) = dest.and_then(|d| d.into_path().ok()) else {
            return; // cancelled
        };
        // On a thread, because the copy is as slow as the backend is far away.
        // A local file arrives at disk speed and the dialog's own callback
        // could carry it; a backend reading over a network takes as long as
        // the file is big, and this callback runs where a click is answered —
        // the window would stop repainting for the length of the transfer,
        // including the part of it that would have said what was going on.
        thread::spawn(move || {
            // Streamed through the backend rather than `fs::copy`, which only a
            // local path could satisfy. `fs::copy` also carried the permission
            // bits, so a downloaded script stayed runnable — restore them from
            // the backend's metadata where it knows them.
            let mode = vfs.metadata(&src).ok().and_then(|m| m.mode);
            let copied = vfs.open(&src).and_then(|mut from| {
                fs::File::create(&dest).and_then(|mut to| io::copy(&mut from, &mut to))
            });
            match copied {
                Err(e) => say(&app, &format!("Could not save {}: {e}", dest.display())),
                Ok(_) => {
                    #[cfg(unix)]
                    if let Some(mode) = mode {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(mode));
                    }
                    #[cfg(not(unix))]
                    let _ = mode;
                }
            }
        });
    });
    }
}

/// The phone's Download: a copy into Files, under the name it already has.
///
/// No dialog, because there is nothing to ask — one folder is browsable and it
/// is the one the reader is offered as a Place. The copy is streamed through the
/// backend the same way the desktop's is, and for the same reason: this runs
/// after a click, and a file is as slow as the machine it is on is far away.
#[cfg(mobile)]
fn save_into_files(app: &AppHandle, target: &treeserve::Resolved, root: &treeserve::Root) {
    let Some(dir) = app_storage_dir(app) else {
        say(app, "This device gave the app no storage to save into.");
        return;
    };
    let name = target.rel.last().cloned().unwrap_or_else(|| "download".into());
    let dest = free_name(&dir, &name);
    let vfs = Arc::clone(&root.vfs);
    let src = target.path.clone();
    let app = app.clone();
    thread::spawn(move || {
        let copied = vfs.open(&src).and_then(|mut from| {
            fs::File::create(&dest).and_then(|mut to| io::copy(&mut from, &mut to))
        });
        match copied {
            Ok(_) => say_ok(
                &app,
                &format!(
                    "Saved to Files as {}",
                    dest.file_name().unwrap_or_default().to_string_lossy()
                ),
            ),
            Err(e) => {
                let _ = fs::remove_file(&dest);
                say(&app, &format!("Could not save {name}: {e}"));
            }
        }
    });
}

/// `name`, or `name (2)` and upwards where that is taken. The extension stays on
/// the end where there is one — `report (2).pdf`, not `report.pdf (2)` — because
/// the thing that opens it reads the end of the name and not the middle.
#[cfg(mobile)]
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let taken = dir.join(name);
    if !taken.exists() {
        return taken;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        // A leading dot is the whole name, not an extension: `.bashrc` is not a
        // file called nothing of type bashrc.
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    for n in 2..1000 {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    taken
}

/// The other half of [`say`], for something that went right.
#[cfg(mobile)]
fn say_ok(app: &AppHandle, msg: &str) {
    let back = app.clone();
    let msg = msg.to_string();
    let _ = app.run_on_main_thread(move || notify(&back, &msg));
}

/// A dialog from a thread: the window's own is the main thread's to open.
fn say(app: &AppHandle, msg: &str) {
    let back = app.clone();
    let msg = msg.to_string();
    let _ = app.run_on_main_thread(move || fail(&back, &msg, false));
}

/// Minimal percent-decoding for a single URL path segment.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    while i < bytes.len() {
        match (bytes[i], bytes.get(i + 1).copied(), bytes.get(i + 2).copied()) {
            (b'%', Some(h), Some(l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push(hex(h).unwrap() * 16 + hex(l).unwrap());
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Handles the page's own controls, which are links rather than script: the
/// back button, the picker button, and the two lists that re-root. Returns
/// whether the navigation was one of ours and should therefore be cancelled.
fn shell_action(app: &AppHandle, url: &tauri::Url) -> bool {
    match url.path() {
        "/.ts/open" => ask_for_folder(app.clone(), false),
        // The other end of having a folder open. What was open is in Recent, so
        // there is nothing to confirm; and a session an embedder opened for it is
        // not this server's to close — the row that opened it will open it again.
        "/.ts/close" => close_folder(app),
        // The Refresh flag. A link to the page it is on would have done the same
        // work on the server and cost the reader their place in it: a navigation
        // starts at the top of the document and leaves the old page behind in the
        // history. A reload keeps the scroll and keeps Back meaning the folder
        // before this one.
        "/.ts/reload" => eval(app, "location.reload()"),
        // The Print flag, and Ctrl+P, which in this window is not the engine's to
        // answer: there is no menu here that would have carried it. The page's
        // print rules are what makes the result worth looking at — no header, no
        // status line, no pane, and the light palette whatever is on screen.
        "/.ts/print" => eval(app, "window.print()"),
        // The theme / line-number / pane toggles, and the tree's disclosure
        // arrows. Both answer 303-and-a-cookie, and a 303 is the one reply a
        // custom scheme cannot carry out: WebKit takes the empty body and stays
        // on the page it was showing, so a new preference only turned up on
        // whatever page came next. Over the HTTP build the network stack
        // followed it and the page changed under your hand, which is the
        // behaviour to restore. What is stored is still the router's call — only
        // going back to the page is this side's.
        "/.ts/set" | "/.ts/tree" => set_pref(app, url),
        // A Recent row's own button. Nothing to open and nothing to wait for, so
        // unlike its neighbours this one answers here and reloads: the pane is
        // part of the page, and the row has to leave it.
        // The header's own control, which acts on whatever is open — so unlike its
        // neighbours it needs no path, and a reload is what puts the new state on
        // screen: the flag is part of the page and has to change with it.
        "/.ts/pin" => off_the_callback(app, |app| {
            if let Some(serving) = app.try_state::<Serving>()
                && let Some(root) = serving.state().cfg.root()
            {
                let label = serving.state().cfg.root_name();
                pin_root_id(app, &root.id, label);
                eval(app, "location.reload()");
            }
        }),
        // Two callers: the same control once the root is pinned, with nothing to
        // say, and a row in the list, which names the one it means.
        "/.ts/unpin" => {
            let id = match url.query_pairs().find(|(k, _)| k == "path") {
                Some((_, path)) => Some(path.trim().to_string()),
                None => app
                    .try_state::<Serving>()
                    .and_then(|s| s.state().cfg.root())
                    .map(|root| root.id.clone()),
            };
            if let Some(id) = id {
                off_the_callback(app, move |app| {
                    unpin_root_id(app, &id);
                    eval(app, "location.reload()");
                });
            }
        }
        "/.ts/forget" => match url.query_pairs().find(|(k, _)| k == "path") {
            Some((_, path)) => {
                let id = path.trim().to_string();
                off_the_callback(app, move |app| {
                    forget_root_id(app, &id);
                    eval(app, "location.reload()");
                });
            }
            None => fail(app, "No folder in that link.", false),
        },
        // Recent; a Place is the same but not worth remembering, since the pane
        // already lists it. Both carry a path we rendered ourselves, though
        // `open_root` still checks it — a remembered folder can go away.
        "/.ts/root" | "/.ts/place" => match url.query_pairs().find(|(k, _)| k == "path") {
            // Ours, and already in memory: no canonicalizing, no thread, and no
            // status to record afterwards.
            Some((_, path)) if usage::claims(path.trim()) => usage::open(app),
            // Not a path: an embedder's kind of root, if one of its openers
            // owns it. Coercing it into a PathBuf would "open" a folder named
            // `ssh:…`, fail, and grey a healthy entry with a status nothing
            // ever corrects — so where nobody claims it, say what actually
            // happened instead.
            Some((_, path)) if !treeserve::root_id_is_local(path.trim()) => {
                let id = path.trim().to_string();
                match opener_for(app, &id) {
                    Some(opener) => open_by(app, opener, id, url.path() == "/.ts/root"),
                    None => fail(app, &format!("Nothing here can open {id}."), false),
                }
            }
            Some((_, path)) => {
                let dir = PathBuf::from(path.trim());
                open_root(app, dir, url.path() == "/.ts/root");
            }
            None => fail(app, "No folder in that link.", false),
        },
        _ => return false,
    }
    true
}

/// Stores what a preference route asked for and puts the page back on screen.
///
/// On a thread of its own, for the reason `open_root` is: this runs from
/// `on_navigation`, and on Android that callback is the Java UI thread. The
/// `w.url()` below is not a local read there — it posts to wry's pipe and waits
/// ten seconds for an answer only that same thread can give, so tapping the
/// theme flag froze the window until Android offered to kill the app. From a
/// thread of our own every call completes, because the looper is free.
///
/// The getter could have gone instead of moving: the toggle is always clicked on
/// the page it applies to, so the comparison below has one answer today. It is
/// kept because the `back=` it exists for is a link somebody may yet draw, and a
/// thread costs a click.
fn set_pref(app: &AppHandle, url: &tauri::Url) {
    let app = app.clone();
    let url = url.clone();
    thread::spawn(move || set_pref_now(&app, &url));
}

fn set_pref_now(app: &AppHandle, url: &tauri::Url) {
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    let Some(jar) = app.try_state::<JarState>() else {
        return;
    };
    // The jar goes in, not just out: `/.ts/tree` changes one entry of a set it
    // reads from the `Cookie`, and with no cookie to read it wrote a set of one —
    // so opening a directory quietly shut every other one, and a second level
    // could never appear. `/.ts/set` never noticed, its keys being independent.
    let reply = treeserve::handle(
        serving.state(),
        &treeserve::Req {
            url: path_and_query(url),
            headers: with_jar(Vec::new(), &jar.0),
            is_get: true,
        },
    );
    jar.0.store(&reply);
    let Some(back) = header(&reply, "Location") else {
        return;
    };
    let here = app
        .get_webview_window(WINDOW)
        .and_then(|w| w.url().ok())
        .map(|u| path_and_query(&u));
    // A toggle is always clicked on the page it applies to, so this is the usual
    // way through: a reload keeps the reader's place in a long listing and keeps
    // Back meaning the folder before this one, the same reasoning `/.ts/reload`
    // is written down under. The navigation is for a `back=` that points
    // somewhere else, which nothing draws today.
    match here.as_deref() == Some(back.as_str()) {
        true => eval(app, "location.reload()"),
        false => replace_page(app, &format!("{}{}", serving.origin, back)),
    }
}

/// A URL as the router speaks it: path and query, no scheme, no host.
fn path_and_query(url: &tauri::Url) -> String {
    match url.query() {
        Some(q) => format!("{}?{}", url.path(), q),
        None => url.path().to_string(),
    }
}

fn header(reply: &treeserve::Reply, name: &str) -> Option<String> {
    reply
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

/// Puts the window back on the start page.
/// Runs one of `shell_action`'s answers away from the thread that asked for it.
///
/// The pinned and recent lists live in the config directory, and asking where
/// that is is a platform call on Android — dispatched, with no timeout, to the
/// Java UI thread that `on_navigation` is already running on. Pinning a folder
/// there would have hung the app for good. See the threading rules in
/// `docs/architecture.md`.
fn off_the_callback(app: &AppHandle, f: impl FnOnce(&AppHandle) + Send + 'static) {
    let app = app.clone();
    thread::spawn(move || f(&app));
}

fn close_folder(app: &AppHandle) {
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    serving.state().cfg.close_root();
    replace_page(app, &serving.entry);
}

/// Puts a URL on screen in place of the page that is there, rather than on top
/// of it.
///
/// Every navigation this shell makes for itself is a change of what is being
/// shown, not a step forward through anything: re-rooting, a wait page, putting
/// back the page a cancelled dial came from. A `navigate` would push an entry
/// for each, and since every page of this window wears the same address, that
/// entry is a copy of the page you are already on — Back into it re-renders
/// whatever root is current now. `location.replace` changes the page and leaves
/// the history alone, which is what all of them want.
///
/// Public because a downstream shell navigates the same window for the same
/// reasons, and getting this wrong is invisible until someone presses Back —
/// or, on Android, swipes and finds the gesture doing nothing several times
/// before it leaves.
pub fn replace_page(app: &AppHandle, url: &str) {
    let Some(win) = app.get_webview_window(WINDOW) else {
        return;
    };
    let loaded = app
        .try_state::<Serving>()
        .is_some_and(|s| s.loaded.load(Ordering::Relaxed));
    match loaded {
        true => {
            let _ = win.eval(&format!("location.replace('{}')", js_quoted(url)));
        }
        // Nothing loaded yet to run that in — the window is still fetching its
        // own first page. A navigation supersedes a load that has not committed,
        // so this leaves one entry too.
        false => {
            if let Ok(url) = url.parse() {
                let _ = win.navigate(url);
            }
        }
    }
}

/// `HeaderFlag` is four strings and an option; the embedder's copy stays theirs.
fn clone_flag(f: &treeserve::HeaderFlag) -> treeserve::HeaderFlag {
    treeserve::HeaderFlag {
        href: f.href.clone(),
        icon: f.icon.clone(),
        label: f.label.clone(),
        title: f.title.clone(),
        roots: f.roots.clone(),
    }
}

/// A string as a single-quoted JavaScript literal. Our own URLs only, which is
/// why this is three characters rather than a JSON encoder — but a scheme base
/// is configurable, so it is not nothing.
fn js_quoted(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn eval(app: &AppHandle, js: &str) {
    if let Some(w) = app.get_webview_window(WINDOW) {
        let _ = w.eval(js);
    }
}

/// Fixed shortcuts for the pane's Places list.
///
/// The shell resolves these because it is the side that knows the platform:
/// where the home directory is, and what stands in for "everything else" —
/// drive roots on Windows, `/` elsewhere. That last entry is the one the GTK
/// picker calls "Other Locations" and the Windows picker calls "This PC";
/// having our own means it is in the same place on both.
fn places(app: &AppHandle) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();

    // First on a phone, and on Android the only one: a folder of the app's own is
    // the one a phone has that nobody has to grant.
    #[cfg(mobile)]
    if let Some(dir) = app_storage_dir(app) {
        // "Files", not "App storage": what the row opens is a folder for the
        // reader's own things — what they downloaded, what another app sent
        // here — and not a window onto the app's private root, which is where
        // its config, its keys and whatever secrets it keeps all live.
        out.push(("Files".to_string(), dir));
    }

    // Everywhere but Android, where all three of these name something other than
    // what the label says. `document_dir()` and `download_dir()` resolve to
    // `getExternalFilesDir(…)` — the app's own external folder wearing the user's
    // folder names, so the row would promise Documents and open an empty one of
    // ours — and `home_dir()` is `getExternalStorageDirectory()`, the volume root,
    // which scoped storage lets nobody list. There is one way to a folder of the
    // user's on that platform and it is asking for it, which is the picker's job.
    //
    // iOS is not in that position and is not excluded with it: `$HOME` there is
    // the app's own sandbox, so Home and Documents are folders this app may read.
    #[cfg(not(target_os = "android"))]
    {
        let p = app.path();
        let mut named: Vec<(&str, Option<PathBuf>)> = vec![("Home", p.home_dir().ok())];
        // No desktop on a device with no desktop.
        #[cfg(desktop)]
        named.push(("Desktop", p.desktop_dir().ok()));
        named.push(("Documents", p.document_dir().ok()));
        named.push(("Downloads", p.download_dir().ok()));

        out.extend(named.into_iter().filter_map(|(label, dir)| {
            let dir = dir?;
            // Dropped if it is not there; offering a folder that does not exist
            // helps nobody. A sandbox path is as cheap and as honest to stat as a
            // desktop one — `$HOME/Downloads` is a folder nothing on iOS creates,
            // and a row for it would never come good. The probe this used to skip
            // for all of mobile was skipped for Android's reasons, and Android no
            // longer reaches here.
            if !dir.is_dir() {
                return None;
            }
            Some((label.to_string(), dir))
        }));
    }

    // Which letters exist, asked of the system rather than of the drives. The
    // obvious loop — `is_dir()` on A:\ through Z:\ — puts a `GetFileAttributesW`
    // on each of the 26, and a letter that exists but is not ready does not fail,
    // it *waits*: a mapped drive whose server is gone waits out the SMB
    // redirector timeout, and a spun-down external disk waits for the platters.
    // Measured on a machine with one disconnected mapping, `Z:` to a NAS that was
    // off: 21.1 seconds for the 26 probes. All of it fell between choosing a
    // folder and this window existing, because that is where `start` runs, and
    // only on the first open — every later one re-roots a window that is already
    // up. `GetLogicalDrives` is a bitmask out of the object namespace and touches
    // no device, so it cannot stall.
    //
    // The trade is that a letter which is present but unreachable is now listed.
    // That is the better half of it: the old probe paid for the discovery every
    // time and then hid the drive, where this pays nothing and answers when the
    // drive is actually asked for — which is also what Explorer does.
    #[cfg(windows)]
    {
        let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
        for letter in b'A'..=b'Z' {
            if mask & (1 << (letter - b'A')) != 0 {
                let c = letter as char;
                out.push((format!("{c}:"), PathBuf::from(format!("{c}:\\"))));
            }
        }
    }
    // Not on a phone: `/` is readable enough to list and holds nothing a user
    // put there, so it offers a tour of the OS in place of their own files.
    #[cfg(all(not(windows), desktop))]
    out.push(("Filesystem".to_string(), PathBuf::from("/")));

    out
}

/// The window's title for a page the tree served: what the root is called, if
/// whoever re-rooted said, and otherwise the folder it ends in.
fn window_title(cfg: &Config) -> String {
    // Whatever this program is called, which is not necessarily this crate.
    let app = cfg
        .app_name
        .clone()
        .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string());
    let Some(root) = cfg.root() else {
        // Nothing open: the app is all there is to name.
        return app;
    };
    if let Some(name) = cfg.root_name() {
        return format!("{name} — {app}");
    }
    match treeserve::leaf_of(&root.id) {
        Some(name) => format!("{name} — {app}"),
        // A drive or a share: no last component, so say which one it is.
        None => format!("{} — {app}", root.id),
    }
}

/// Whether an extension declared this URL's origin. See
/// [`ShellExt::allowed_origins`] for the two entry shapes.
fn origin_allowed(allowed: &[String], url: &tauri::Url) -> bool {
    allowed.iter().any(|a| match a.strip_suffix(':') {
        Some(scheme) if !a.contains('/') => url.scheme() == scheme,
        _ => url.origin().ascii_serialization() == *a,
    })
}

/// Says something happened, for actions with no visible result of their own.
fn notify(app: &AppHandle, msg: &str) {
    app.dialog()
        .message(msg)
        .kind(MessageDialogKind::Info)
        .title("treesight")
        .show(|_| {});
}

/// Reports a problem in a native dialog, since a GUI build has nowhere to print.
fn fail(app: &AppHandle, msg: &str, fatal: bool) {
    let app = app.clone();
    let msg = msg.to_string();
    app.dialog()
        .message(msg)
        .kind(MessageDialogKind::Error)
        .title("treesight")
        .show(move |_| {
            if fatal {
                app.exit(1);
            }
        });
}

fn recent_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("recent.txt"))
}

/// Roots served before, newest first, as the RootIds the server's lists and
/// status map speak.
///
/// Nothing here checks that they are still there. It used to, and that was a
/// blocking stat per entry on the way to the picker: one remembered folder on a
/// disconnected network drive and the dialog was twenty seconds late, every
/// launch. `check_roots` finds out afterwards instead, the pane greys out what is
/// gone, and the file loses it so the next launch never lists it.
fn recent(app: &AppHandle) -> Vec<String> {
    let Some(file) = recent_file(app) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(file) else {
        return Vec::new();
    };
    recent_ids(&text)
}

/// The file's lines as RootIds. A file written before the RootId change holds
/// the verbatim form (`\\?\C:\…`), which is a spelling of a local path and not
/// an id, so every line goes through the same normalization a fresh one gets —
/// otherwise the same folder is two entries and neither matches the status map.
/// A remote id passes through untouched: `display_path` only strips a prefix
/// nothing but Windows produces.
fn recent_ids(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| treeserve::util::display_path(Path::new(l)))
        .take(RECENT_MAX)
        .collect()
}

/// The Recent list with `id` at the front: an id already in it moves rather
/// than being added again, and the list keeps its length.
fn with_front(mut list: Vec<String>, id: &str) -> Vec<String> {
    list.retain(|x| x != id);
    list.insert(0, id.to_string());
    list.truncate(RECENT_MAX);
    list
}

/// Moves a RootId to the front of Recent, on disk and in the running server so
/// the next page render shows it. Places are deliberately not fed through here:
/// that list stays fixed.
///
/// Public: a downstream app that re-roots onto its own backend records the root
/// the same way local opens are recorded. The id is whatever that backend calls
/// the root — for a local one, the display-form path.
pub fn remember_root_id(app: &AppHandle, id: &str) {
    let list = with_front(recent(app), id);
    if let Some(serving) = app.try_state::<Serving>() {
        // Whoever got this far has already resolved the root, so this one is
        // known good without anybody having to look again.
        serving.state().cfg.set_root_status(id.to_string(), RootStatus::Ok);
    }
    save_recent(app, list);
}

/// The theme the shell's own pages are being drawn with.
///
/// For an embedder whose pages sit beside them: the toggle writes `ts_theme` to
/// the jar, and nothing outside this crate could read it, so a scheme page of
/// its own had no way to be dark when the tree was. `Auto` is the answer when
/// nobody has chosen — the same answer the pages act on, which is to let
/// `prefers-color-scheme` decide.
pub fn theme<R: tauri::Runtime>(app: &AppHandle<R>) -> ThemeMode {
    if let Some(jar) = app.try_state::<JarState>()
        && let Some(mode) = jar
            .0
            .cookies
            .lock()
            .unwrap()
            .get("ts_theme")
            .and_then(|v| ThemeMode::from_str(v))
    {
        return mode;
    }
    app.try_state::<Serving>()
        .map(|s| s.state().cfg.theme)
        .unwrap_or(ThemeMode::Auto)
}

/// Chooses the theme, for a page of an embedder's own that carries the same
/// control as the tree's header.
///
/// The tree's toggle is a link to `/.ts/set`, which answers with a cookie and a
/// redirect to a *path* — no use to a page on another scheme, which cannot be
/// redirected back to. This is the same write without the round trip; the tree
/// reads it on its next render, and the caller repaints itself.
pub fn set_theme<R: tauri::Runtime>(app: &AppHandle<R>, mode: ThemeMode) {
    if let Some(jar) = app.try_state::<JarState>() {
        jar.0.set("ts_theme", mode.as_str());
    }
}

/// Drops a RootId from Recent — the row's own button, for a folder that moved or
/// a root written down wrong by a bug since fixed. Only the list: the folder is
/// not this shell's to delete, and the page says "Forget" for that reason.
pub fn forget_root_id(app: &AppHandle, id: &str) {
    let mut list = recent(app);
    let before = list.len();
    list.retain(|x| x != id);
    if list.len() == before {
        return;
    }
    save_recent(app, list);
}

fn pinned_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("pinned.txt"))
}

/// The pinned list as it was left. Order is the order they were pinned in, and
/// there is no cap: a list the reader built by hand is not one to truncate
/// behind their back.
fn pinned(app: &AppHandle) -> Vec<treeserve::Pin> {
    let Some(file) = pinned_file(app) else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(file) else {
        return Vec::new();
    };
    pinned_entries(&text)
}

/// `<label>\t<id>` per line, and a line with no tab is an id nobody named.
///
/// The label goes first on purpose. A RootId may legally contain a tab — a Unix
/// file may be called anything but `/` and NUL — and splitting at the *first* tab
/// then hands the whole of the rest back as the id, whatever is in it. The other
/// way round, an id with a tab in it would take the label's place and the entry
/// would come back as a different root. Labels are ours to write, so the one
/// character that cannot appear in one is a tab.
fn pinned_entries(text: &str) -> Vec<treeserve::Pin> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| match line.split_once('\t') {
            Some((label, id)) => treeserve::Pin {
                id: id.to_string(),
                label: Some(label.to_string()),
            },
            None => treeserve::Pin {
                id: line.trim().to_string(),
                label: None,
            },
        })
        .collect()
}

/// Pins the root that is open, under the name the window is calling it.
///
/// Public for the same reason `remember_root_id` is: a downstream app that serves
/// its own backend pins it the same way, and the name matters more there — a
/// grant or a bookmark has an id nothing would want to read.
pub fn pin_root_id(app: &AppHandle, id: &str, label: Option<String>) {
    let mut list = pinned(app);
    if list.iter().any(|p| p.id == id) {
        return;
    }
    list.push(treeserve::Pin {
        id: id.to_string(),
        label: label.map(|l| l.replace('\t', " ")),
    });
    save_pinned(app, list);
}

/// Drops a root from the pinned list — the header's own control, and each row's.
/// Only the list: the folder is not this shell's to delete.
pub fn unpin_root_id(app: &AppHandle, id: &str) {
    let mut list = pinned(app);
    let before = list.len();
    list.retain(|p| p.id != id);
    if list.len() == before {
        return;
    }
    save_pinned(app, list);
}

fn save_pinned(app: &AppHandle, list: Vec<treeserve::Pin>) {
    let text: String = list
        .iter()
        .map(|p| match &p.label {
            Some(label) => format!("{label}\t{}\n", p.id),
            None => format!("{}\n", p.id),
        })
        .collect();
    if let Some(serving) = app.try_state::<Serving>() {
        serving.state().cfg.set_pinned(list);
    }
    let Some(file) = pinned_file(app) else { return };
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(file, text);
}

/// The list, in the running server and on disk. Both are the same order, and the
/// pane reads the first of them on the next render.
fn save_recent(app: &AppHandle, list: Vec<String>) {
    if let Some(serving) = app.try_state::<Serving>() {
        serving.state().cfg.set_recent(list.clone());
    }
    let Some(file) = recent_file(app) else { return };
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    // One id per line, which is the same string the pane shows and the status
    // map is keyed by, so the three never disagree about which root is which.
    let text: String = list.iter().map(|id| format!("{id}\n")).collect();
    let _ = fs::write(file, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_cookie_is_a_name_a_value_and_nothing_else() {
        assert_eq!(
            cookie_pair("ts_theme=dark; Path=/; Max-Age=31536000; SameSite=Lax"),
            Some(("ts_theme".into(), "dark".into()))
        );
        assert_eq!(cookie_pair("ts_sidebar=0"), Some(("ts_sidebar".into(), "0".into())));
        assert_eq!(cookie_pair("nonsense"), None);
        assert_eq!(cookie_pair("=0"), None);
    }

    /// The whole round trip the toggles make, with the webview's missing jar
    /// standing in for: click `/.ts/set`, then ask for the page again and get
    /// the preference back. Everything here but the webview itself.
    #[test]
    fn a_toggle_survives_the_trip_back_to_the_next_page() {
        let dir = std::env::temp_dir().join(format!("treesight-jar-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut cfg = Config::new(dir.clone());
        cfg.app_ui = true;
        let state = treeserve::state_for(cfg);
        let jar = Jar::default();
        let html = || treeserve::Req {
            url: "/".to_string(),
            headers: with_jar(vec![("Accept".into(), "text/html".into())], &jar),
            is_get: true,
        };

        // Nothing stored: the defaults, and a pane.
        let before = match treeserve::handle(&state, &html()).body {
            treeserve::Body::Text(t) => t,
            _ => panic!("a listing is text"),
        };
        assert!(before.contains("<html lang=\"en\">"), "no theme yet");
        assert!(before.contains("<nav"), "pane is on by default");

        // The toggles, as the header draws them.
        for q in ["/.ts/set?theme=dark&back=/", "/.ts/set?sidebar=0&back=/"] {
            let reply = treeserve::handle(
                &state,
                &treeserve::Req {
                    url: q.to_string(),
                    headers: Vec::new(),
                    is_get: true,
                },
            );
            assert_eq!(reply.status, 303);
            jar.store(&reply);
        }

        let after = match treeserve::handle(&state, &html()).body {
            treeserve::Body::Text(t) => t,
            _ => panic!("a listing is text"),
        };
        assert!(after.contains("<html lang=\"en\" data-theme=\"dark\">"), "{after}");
        // The pane's markup stays whichever way the switch went — the drawer
        // needs something to slide in, and only the stylesheet knows whether
        // this window is wide enough for the switch to have meant anything — so
        // the class on the body is where the answer is.
        assert!(after.contains("class=\"app nopane\""), "{after}");

        fs::remove_dir_all(&dir).unwrap();
    }

    /// What `set_pref` hands the router. A route that changes one entry of a set
    /// it reads from the `Cookie` — `/.ts/tree` — gets nothing right without this,
    /// and gets it wrong quietly: it writes back a set of one.
    #[test]
    fn a_route_the_shell_answers_is_given_the_jar() {
        let jar = Jar::default();
        jar.store(&treeserve::Reply {
            status: 303,
            headers: vec![("Set-Cookie".into(), "ts_open=src|src%2Fbin".into())],
            body: treeserve::Body::Empty,
        });
        assert_eq!(
            with_jar(Vec::new(), &jar),
            vec![("Cookie".to_string(), "ts_open=src|src%2Fbin".to_string())]
        );
    }

    /// Both jars holding an answer is a Windows shape: its own store kept what
    /// the toggles wrote before this shell had a jar, and nothing writes to it
    /// now, so the stale one must not win.
    #[test]
    fn the_jar_outvotes_a_cookie_the_webview_kept() {
        let jar = Jar::default();
        jar.store(&treeserve::Reply {
            status: 303,
            headers: vec![("Set-Cookie".into(), "ts_theme=light".into())],
            body: treeserve::Body::Empty,
        });
        let headers = with_jar(
            vec![
                ("Accept".into(), "text/html".into()),
                ("Cookie".into(), "ts_theme=dark; ts_ln=0".into()),
            ],
            &jar,
        );
        // Its own opinion is kept where the jar has none (`ts_ln`), overruled
        // where it has one (`ts_theme`), and the rest of the headers survive.
        assert!(headers.contains(&("Accept".to_string(), "text/html".to_string())));
        assert_eq!(
            headers
                .iter()
                .find(|(k, _)| k == "Cookie")
                .map(|(_, v)| v.as_str()),
            Some("ts_ln=0; ts_theme=light")
        );
    }

    #[test]
    fn the_jar_reads_back_what_it_wrote() {
        let jar = Jar::default();
        assert!(jar.cookies.lock().unwrap().is_empty());
        jar.store(&treeserve::Reply {
            status: 303,
            headers: vec![
                ("Set-Cookie".into(), "ts_theme=dark; Path=/".into()),
                ("Location".into(), "/".into()),
            ],
            body: treeserve::Body::Empty,
        });
        assert_eq!(cookie_list(&jar.cookies.lock().unwrap()), "ts_theme=dark");
        // A second toggle adds to the jar rather than replacing it, and the same
        // key set twice keeps the later answer.
        jar.store(&treeserve::Reply {
            status: 303,
            headers: vec![
                ("Set-Cookie".into(), "ts_sidebar=0".into()),
                ("Set-Cookie".into(), "ts_theme=light".into()),
            ],
            body: treeserve::Body::Empty,
        });
        assert_eq!(
            cookie_list(&jar.cookies.lock().unwrap()),
            "ts_sidebar=0; ts_theme=light"
        );
        assert_eq!(jar_from_text(&jar_text(&jar.cookies.lock().unwrap())), {
            let mut want = BTreeMap::new();
            want.insert("ts_sidebar".to_string(), "0".to_string());
            want.insert("ts_theme".to_string(), "light".to_string());
            want
        });
    }

    /// The distinction the pane makes, and the one `ErrorKind` cannot make on its
    /// own: a folder someone deleted is gone, and a share whose host is off is not
    /// gone, it is unreachable. Windows answers the second with `ERROR_BAD_NETPATH`,
    /// which std reports as `NotFound` — the same kind as the first.
    #[test]
    fn deleted_is_missing_and_a_dead_mount_is_not() {
        // ENOENT, and ERROR_FILE_NOT_FOUND, which share the number 2.
        assert_eq!(classify(&io::Error::from_raw_os_error(2)), RootStatus::Missing);

        // ERROR_BAD_NETPATH on Windows, ESTALE elsewhere: the mount is there and
        // the server is not.
        let dead = if cfg!(windows) { 53 } else { 116 };
        assert_eq!(
            classify(&io::Error::from_raw_os_error(dead)),
            RootStatus::Unreachable
        );
    }

    /// An allowlist that matched by prefix would wave
    /// `http://telesight.localhost.evil.com` through on the strength of
    /// `http://telesight.localhost`; equality on the serialized origin (or the
    /// whole scheme, for opaque-origin custom protocols) does not.
    #[test]
    fn lookalike_origins_stay_outside() {
        let allowed = vec!["telesight:".to_string(), "http://telesight.localhost".to_string()];
        let u = |s: &str| tauri::Url::parse(s).unwrap();
        assert!(origin_allowed(&allowed, &u("telesight://term")));
        assert!(origin_allowed(&allowed, &u("http://telesight.localhost/term.html")));
        assert!(!origin_allowed(&allowed, &u("http://telesight.localhost.evil.com/x")));
        assert!(!origin_allowed(&allowed, &u("http://evil.com/telesight.localhost")));
        assert!(!origin_allowed(&allowed, &u("https://telesight.localhost/x")));
    }

    /// What the Recent file holds and what the list speaks are now the same
    /// thing — RootIds — and the three cases that used to disagree: a reopened
    /// root moves to the front instead of doubling, a remote id comes back out
    /// spelled exactly as it went in, and a line written before ids existed
    /// holds a Windows verbatim path, which is the same root under a spelling
    /// nothing else in the app uses.
    #[test]
    fn recent_is_a_list_of_ids_and_a_reopened_one_moves() {
        let ids = recent_ids("C:\\Users\\x\r\n\\\\?\\C:\\work\n\n  ssh:prod:/var/www  \n");
        assert_eq!(ids, [r"C:\Users\x", r"C:\work", "ssh:prod:/var/www"]);

        assert_eq!(
            with_front(ids.clone(), "ssh:prod:/var/www"),
            ["ssh:prod:/var/www", r"C:\Users\x", r"C:\work"]
        );
        // The verbatim line and the id it normalizes to are one entry, not two.
        assert_eq!(with_front(ids, r"C:\work").len(), 3);

        let full: Vec<String> = (0..RECENT_MAX).map(|i| format!("/d{i}")).collect();
        assert_eq!(with_front(full, "/new").len(), RECENT_MAX);
    }

    /// The line between "probe it" and "leave it to whoever brought it" is
    /// treeserve's `root_id_is_local`; this pins the cases the shell cares
    /// about (probing, pruning) so a grammar change there fails loudly here.
    #[test]
    fn drive_letters_are_local_and_schemes_are_not() {
        assert!(treeserve::root_id_is_local("/home/x/mix"));
        assert!(treeserve::root_id_is_local(r"C:\Users\x"));
        assert!(treeserve::root_id_is_local(r"\\server\share"));
        assert!(treeserve::root_id_is_local("/odd:name/dir"));
        assert!(!treeserve::root_id_is_local("ssh:prod-web:/var/www"));
        assert!(!treeserve::root_id_is_local("s3:bucket:/data"));
    }

    /// A RootId may contain a tab, so the label goes first and the split is the
    /// first one: whatever follows is the id, verbatim.
    #[test]
    fn a_pinned_line_hands_the_id_back_whole() {
        let list = pinned_entries("Home of it all\t/home/x\n/home/y\nOdd\t/tmp/a\tb\n\n");
        let seen: Vec<(Option<&str>, &str)> = list
            .iter()
            .map(|p| (p.label.as_deref(), p.id.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (Some("Home of it all"), "/home/x"),
                (None, "/home/y"),
                (Some("Odd"), "/tmp/a\tb"),
            ]
        );
    }

}
