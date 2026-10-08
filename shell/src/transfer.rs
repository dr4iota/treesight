//! Writing into the served tree, and moving bytes with the status line
//! saying so: **New folder**, **Upload**, **Delete**, a backend's own verbs on
//! a row, and the progress Download shares.
//!
//! Every write starts here, from a link the shell claims — the server has no
//! route that writes, so a page served over a network can never do it. Each
//! link carries [`treeserve::Config::action_token`]; one without it, say from
//! a README, is refused before anything is touched.
//!
//! Progress is drawn into the page's own status line by the window script
//! (`PROGRESS` in `lib.rs`), which the tree's pages carry like the shortcuts.
//! The pages themselves run no script; this is the shell reaching in, the way
//! a status note is.

#[cfg(desktop)]
use std::fs;
use std::io::{self, Read, Write};
#[cfg(desktop)]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use treeserve::util::percent_encode;
use treeserve::{Vfs, VfsPath};

use crate::{committed, eval, fail, js_quoted, notify, replace_page, Serving, WINDOW};

/// One transfer at a time: two would share one status line and one Cancel.
static BUSY: AtomicBool = AtomicBool::new(false);

/// How much is moved between two looks at Cancel and two updates of the line.
const CHUNK: usize = 256 * 1024;
/// How often the line is redrawn at most. Every chunk was a script call into
/// the page per quarter-megabyte, which on a fast disk is thousands a second.
const TICK: Duration = Duration::from_millis(150);

/// `/.ts/cancel`: the status line's Cancel. The transfer stops after the chunk
/// in flight and cleans up after itself.
pub(crate) fn cancel() {
    treeserve::vfs::cancel::ask();
}

/// The transfer lock, released when dropped.
pub(crate) struct Turn(());

impl Turn {
    pub(crate) fn take() -> Option<Turn> {
        BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| {
                treeserve::vfs::cancel::clear();
                Turn(())
            })
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
        treeserve::vfs::cancel::clear();
    }
}

/// The status line, from any thread. `None` clears it.
pub(crate) fn progress(app: &AppHandle, line: Option<(&str, f64)>) {
    // Into the page on screen only: script sent into one that is about to be
    // replaced runs there and vanishes with it.
    if !committed(app) {
        return;
    }
    let Some(win) = app.get_webview_window(WINDOW) else {
        return;
    };
    let js = match line {
        Some((text, frac)) => format!(
            "window.__tsProgress && window.__tsProgress('{}', {})",
            js_text(text),
            frac.clamp(0.0, 1.0)
        ),
        None => "window.__tsProgress && window.__tsProgress(null)".to_string(),
    };
    let _ = win.eval(&js);
}

fn js_text(s: &str) -> String {
    js_quoted(s).replace('\n', " ").replace('\r', " ")
}

/// Megabytes, to one place, the way the line reads them.
fn mb(n: u64) -> String {
    format!("{:.1}", n as f64 / 1_048_576.0)
}

/// Copies `from` into `to` through the status line: [`pump_with`], reporting
/// into the page.
pub(crate) fn pump(
    app: &AppHandle,
    what: &str,
    total: Option<u64>,
    from: &mut dyn Read,
    to: &mut dyn Write,
) -> io::Result<u64> {
    pump_with(what, total, from, to, &mut |line, frac| progress(app, Some((line, frac))))
}

/// Copies `from` into `to`, saying how far it has got as `what` — *Uploading
/// 2 of 3 · photo.jpg* — at most every [`TICK`], and stopping with
/// `Interrupted` when Cancel is pressed. `total` is what the bar is a fraction
/// of; the bytes are counted either way.
fn pump_with(
    what: &str,
    total: Option<u64>,
    from: &mut dyn Read,
    to: &mut dyn Write,
    report: &mut dyn FnMut(&str, f64),
) -> io::Result<u64> {
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    let mut last = Instant::now() - TICK;
    loop {
        if treeserve::vfs::cancel::asked() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            // A backend that was waiting inside the read stops for Cancel too
            // (`treeserve::vfs::cancel`); whatever it called the stop, it is
            // the reader's Cancel, said the one way the callers read it.
            Err(_) if treeserve::vfs::cancel::asked() => {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        to.write_all(&buf[..n])?;
        done += n as u64;
        if last.elapsed() >= TICK {
            last = Instant::now();
            let (line, frac) = match total {
                Some(t) if t > 0 => (
                    format!("{what} · {} of {} MB", mb(done), mb(t)),
                    done as f64 / t as f64,
                ),
                _ => (format!("{what} · {} MB", mb(done)), 0.0),
            };
            report(&line, frac);
        }
    }
    Ok(done)
}

/// A claimed link's query value.
fn param(url: &tauri::Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

/// The folder a write link names, checked: the token is this run's, the
/// address is inside the served root, it is a folder, and the tree takes
/// writes. `Err` carries what to tell the reader, or nothing when there is
/// nobody to tell.
struct Target {
    vfs: std::sync::Arc<dyn Vfs>,
    dir: VfsPath,
    /// The folder's address, with its slash, for coming back to it.
    href: String,
    origin: String,
}

fn target(app: &AppHandle, url: &tauri::Url) -> Result<Target, Option<String>> {
    let serving = app.try_state::<Serving>().ok_or(None)?;
    let cfg = &serving.state().cfg;
    if param(url, "t").as_deref() != Some(cfg.action_token.as_str()) {
        return Err(Some("That link did not come from this window.".into()));
    }
    let root = cfg.root().ok_or(None)?;
    let href = param(url, "dir").ok_or(None)?;
    let resolved = treeserve::resolve_in_root(root.vfs.as_ref(), &href)
        .map_err(|_| Some(format!("{href} is not in this folder.")))?;
    let is_dir = root
        .vfs
        .metadata(&resolved.path)
        .map(|m| m.is_dir)
        .unwrap_or(false);
    if !is_dir || !root.vfs.writable() {
        return Err(Some(format!("{href} does not take new files.")));
    }
    Ok(Target {
        vfs: std::sync::Arc::clone(&root.vfs),
        dir: resolved.path,
        href: format!("{}/", href.trim_end_matches('/')),
        origin: serving.origin.clone(),
    })
}

/// `/.ts/mkdir`: New folder's form, sent. Makes the folder and goes back to
/// the folder it is in with its row lit, or back to the form with the name
/// kept and the reason under it. Replaced rather than stepped to, so Back
/// does not walk through the form.
pub(crate) fn mkdir(app: &AppHandle, url: &tauri::Url) {
    let app = app.clone();
    let url = url.clone();
    thread::spawn(move || {
        let t = match target(&app, &url) {
            Ok(t) => t,
            Err(Some(why)) => return fail(&app, &why, false),
            Err(None) => return,
        };
        let typed = param(&url, "name").unwrap_or_default();
        let made = treeserve::new_name(&typed)
            .map_err(str::to_string)
            .and_then(|name| match t.vfs.create_dir(&t.dir, name) {
                Ok(()) => Ok(name.to_string()),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    Err(format!("Something named {name} is already here."))
                }
                Err(e) => Err(format!("Could not make {name}: {e}")),
            });
        let back = match made {
            Ok(name) => format!("{}{}?made={}", t.origin, t.href, percent_encode(&name)),
            Err(why) => format!(
                "{}{}?new=folder&name={}&err={}",
                t.origin,
                t.href,
                percent_encode(&typed),
                percent_encode(&why)
            ),
        };
        replace_page(&app, &back);
    });
}

/// One file the reader picked for Upload: the name it goes under, its length
/// when the platform says (for the progress bar), and how to read it — once.
///
/// A path on a desktop; on a phone whatever its platform code opens a
/// `content://` address with. Either way this side only names it and reads it.
pub struct PickedFile {
    pub name: String,
    pub len: Option<u64>,
    #[allow(clippy::type_complexity)]
    pub open: Box<dyn FnOnce() -> io::Result<Box<dyn Read + Send>> + Send>,
}

type Picker = dyn Fn(&AppHandle) -> io::Result<Option<Vec<PickedFile>>> + Send + Sync;

/// The embedder's picker, from `ShellExt::pick_files`, set once at start.
static PICKER: std::sync::OnceLock<Box<Picker>> = std::sync::OnceLock::new();

pub(crate) fn set_picker(pick: Box<Picker>) {
    let _ = PICKER.set(pick);
}

/// `/.ts/upload`: Upload. Files from the embedder's picker where there is one
/// and from the system's own otherwise; then one question if any of their
/// names are taken, the copy with the status line saying how far it is, and
/// the folder again with the new rows lit.
pub(crate) fn upload(app: &AppHandle, url: &tauri::Url) {
    let t = match target(app, url) {
        Ok(t) => t,
        Err(Some(why)) => return fail(app, &why, false),
        Err(None) => return,
    };
    // Asked before the picker, so a reader is not sent to choose files that
    // cannot go — and asked again by `send` when they come back, which is
    // where the turn is taken. Taking it here held it for as long as the
    // picker was up: a download started meanwhile lost its progress and its
    // Cancel, and a second Upload was told something was being sent.
    if BUSY.load(Ordering::SeqCst) {
        return fail(app, BUSY_SAYS, false);
    }
    if PICKER.get().is_some() {
        let app = app.clone();
        thread::spawn(move || {
            let Some(pick) = PICKER.get() else { return };
            match pick(&app) {
                Ok(Some(files)) if !files.is_empty() => send(&app, t, files),
                Ok(_) => {} // cancelled
                Err(e) => fail(&app, &format!("Could not read the picked files: {e}"), false),
            }
        });
        return;
    }
    #[cfg(desktop)]
    pick_on_desktop(app, t);
    #[cfg(mobile)]
    fail(app, "Upload is not available on this device.", false);
}

/// Either way round: a download holds the same line an upload would.
const BUSY_SAYS: &str = "A file is already on its way. Wait for it, or cancel it.";

/// The system's own picker, which on a desktop hands back paths.
#[cfg(desktop)]
fn pick_on_desktop(app: &AppHandle, t: Target) {
    use tauri_plugin_dialog::DialogExt;
    let Some(dialog_turn) = crate::DialogTurn::take() else {
        return;
    };
    let folder = t.dir.segments().last().cloned().unwrap_or_else(|| "this folder".into());
    let back = app.clone();
    let _ = app.run_on_main_thread(move || {
        let mut dialog = back.dialog().file().set_title(format!("Upload to {folder}"));
        if let Some(win) = back.get_webview_window(WINDOW) {
            dialog = dialog.set_parent(&win);
        }
        let app = back.clone();
        dialog.pick_files(move |picked| {
            drop(dialog_turn);
            let files: Vec<PickedFile> = picked
                .unwrap_or_default()
                .into_iter()
                .filter_map(|p| p.into_path().ok())
                .map(picked_path)
                .collect();
            if files.is_empty() {
                return; // cancelled
            }
            thread::spawn(move || send(&app, t, files));
        });
    });
}

#[cfg(desktop)]
fn picked_path(path: PathBuf) -> PickedFile {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let len = fs::metadata(&path).ok().filter(|m| m.is_file()).map(|m| m.len());
    PickedFile {
        name,
        len,
        open: Box::new(move || {
            // Only a regular file is opened: a fifo or a device would block in
            // the open, before Cancel is ever looked at. The rest is only what
            // the refusal says — a path the dialog handed back that is not
            // there read as "not a file", which sent the reader to look at a
            // file that is fine.
            match fs::metadata(&path) {
                Ok(m) if m.is_file() => Ok(Box::new(fs::File::open(&path)?) as Box<dyn Read + Send>),
                Ok(m) if m.is_dir() => Err(io::Error::other("a folder, not a file")),
                Ok(_) => Err(io::Error::other("not a regular file")),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Err(io::Error::other("not found")),
                Err(e) => Err(e),
            }
        }),
    }
}

/// The upload proper, off every callback: the clash question, the copy, and
/// the page again.
fn send(app: &AppHandle, t: Target, files: Vec<PickedFile>) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let Some(_turn) = Turn::take() else {
        return fail(app, BUSY_SAYS, false);
    };
    let mut refused: Vec<String> = Vec::new();
    // Each picked file under the name it will have here, or why it cannot go.
    let mut todo: Vec<(PickedFile, String)> = Vec::new();
    for f in files {
        match treeserve::new_name(&f.name) {
            // Two picked files under one name — from two folders on a desktop,
            // or two a phone's provider could not name — are one name here.
            // The first is the one used; the second is said, not sent over
            // it. Not "sent once": the first may yet be skipped.
            Ok(name) if todo.iter().any(|(_, n)| n == name) => {
                refused.push(format!("{name}: picked twice; the first is the one used"))
            }
            Ok(name) => {
                let name = name.to_string();
                todo.push((f, name));
            }
            Err(why) => refused.push(format!("{}: {why}", f.name)),
        }
    }
    // What is already here, asked once for the whole batch and before
    // anything moves. A folder of that name is never replaced by a file.
    let mut clash: Vec<String> = Vec::new();
    todo.retain(|(_, name)| match t.vfs.metadata(&t.dir.join(name)) {
        // Before the question, so a link is never offered for replacing.
        Ok(_) if t.vfs.is_link(&t.dir.join(name)) => {
            refused.push(format!("{name}: a link of that name is here, and is not written through"));
            false
        }
        Ok(m) if m.is_dir => {
            refused.push(format!("{name}: a folder of that name is here"));
            false
        }
        Ok(_) => {
            clash.push(name.clone());
            true
        }
        Err(_) => true,
    });
    let mut replace = false;
    if !clash.is_empty() {
        let shown = match clash.len() {
            n if n <= 6 => clash.join(", "),
            n => format!("{}, and {} more", clash[..5].join(", "), n - 5),
        };
        let folder = t.dir.segments().last().cloned().unwrap_or_else(|| "this folder".into());
        // One clash is named and asked about as one; several are counted.
        let (head, them) = match (clash.len(), todo.len()) {
            (1, _) => (format!("{} is already in {folder}.", clash[0]), "it with the one"),
            (c, n) if c == n => (format!("All {n} files are already in {folder}."), "them with the ones"),
            (c, n) => (format!("{c} of the {n} files are already in {folder}."), "them with the ones"),
        };
        #[cfg_attr(mobile, allow(unused_mut))]
        let mut ask = app
            .dialog()
            .message(match clash.len() {
                1 => format!("{head}\n\nReplace {them} you picked? A skipped file is left as it is."),
                _ => format!(
                    "{head}\n{shown}\n\nReplace {them} you picked? Skipped files are left as they are."
                ),
            })
            .title("Upload")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Replace".into(),
                match clash.len() {
                    1 => "Skip it".into(),
                    _ => "Skip them".into(),
                },
            ));
        #[cfg(desktop)]
        if let Some(win) = app.get_webview_window(WINDOW) {
            ask = ask.parent(&win);
        }
        replace = ask.blocking_show();
        if !replace {
            todo.retain(|(_, name)| !clash.contains(name));
        }
    }

    let n = todo.len();
    let mut sent: Vec<String> = Vec::new();
    let mut cancelled: Vec<String> = Vec::new();
    // Sent, but with something to say: the backend's own words.
    let mut notes: Vec<String> = Vec::new();
    let mut todo = todo.into_iter().enumerate();
    while let Some((i, (file, name))) = todo.next() {
        let what = format!("Uploading {} of {n} · {name}", i + 1);
        let PickedFile { len, open, .. } = file;
        let res = (|| -> io::Result<Option<String>> {
            let mut from = open()?;
            let mut to = t.vfs.create_file(&t.dir, &name, replace && clash.contains(&name))?;
            // A file that does not finish is the backend's to take away
            // (`WriteFile`): only it knows whether it made a new file or a
            // temporary one beside a file being replaced, and removing "the
            // name" here removed the file the reader had asked to keep.
            match pump(app, &what, len, &mut from, &mut *to) {
                Ok(_) => to.finish(),
                Err(e) => Err(e),
            }
        })();
        match res {
            Ok(note) => {
                notes.extend(note);
                sent.push(name);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                // What was on its way, and what had not started, by name.
                cancelled.push(name);
                cancelled.extend(todo.by_ref().map(|(_, (_, name))| name));
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                refused.push(format!("{name}: taken while it waited, left as it was"))
            }
            Err(e) => refused.push(format!("{name}: {e}")),
        }
    }
    progress(app, None);
    let lit: String = sent
        .iter()
        .map(|s| format!("made={}", percent_encode(s)))
        .collect::<Vec<_>>()
        .join("&");
    let page = match lit.is_empty() {
        true => format!("{}{}", t.origin, t.href),
        false => format!("{}{}?{lit}", t.origin, t.href),
    };
    replace_page(app, &page);
    // A cancel is the reader's own act, so it is reported only where it left
    // something they may not expect: files that did not go. Said as a notice,
    // not as a failure.
    if !notes.is_empty() {
        crate::notify(app, &notes.join("\n"));
    }
    match (cancelled.is_empty(), refused.is_empty()) {
        (true, true) => {}
        (true, false) => fail(app, &format!("Not sent:\n{}", refused.join("\n")), false),
        (false, _) => {
            let mut lines = cancelled;
            lines.extend(refused);
            crate::notify(app, &format!("Upload cancelled. Not sent:\n{}", lines.join("\n")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes arrive whole, the line says how far, and Cancel stops it with
    /// the one error the callers read as "said no", not as a failure.
    #[test]
    fn a_copy_reports_and_stops_when_cancelled() {
        let data = vec![7u8; CHUNK * 3 + 10];
        let mut out = Vec::new();
        let mut lines = Vec::new();
        let n = pump_with("Saving x", Some(data.len() as u64), &mut &data[..], &mut out, &mut |l, f| {
            lines.push((l.to_string(), f))
        })
        .unwrap();
        assert_eq!(n, data.len() as u64);
        assert_eq!(out, data);
        assert!(lines[0].0.starts_with("Saving x · 0.2 of 0.8 MB"), "{lines:?}");

        let turn = Turn::take().expect("nothing else is moving");
        cancel();
        let mut out = Vec::new();
        let e = pump_with("Saving x", None, &mut &data[..], &mut out, &mut |_, _| {}).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
        // Letting go of the turn clears Cancel for the next transfer.
        drop(turn);
        assert!(!treeserve::vfs::cancel::asked());
    }
}

/// The row a row link names, checked as [`target`] checks a folder: this run's
/// token, and an address inside the served root.
struct Row {
    vfs: std::sync::Arc<dyn Vfs>,
    path: VfsPath,
    name: String,
    is_dir: bool,
}

fn row(app: &AppHandle, url: &tauri::Url) -> Result<Row, Option<String>> {
    let serving = app.try_state::<Serving>().ok_or(None)?;
    let cfg = &serving.state().cfg;
    if param(url, "t").as_deref() != Some(cfg.action_token.as_str()) {
        return Err(Some("That link did not come from this window.".into()));
    }
    let root = cfg.root().ok_or(None)?;
    let href = param(url, "path").ok_or(None)?;
    let resolved = treeserve::resolve_in_root(root.vfs.as_ref(), &href)
        .map_err(|_| Some(format!("{href} is not in this folder.")))?;
    let is_dir = root
        .vfs
        .metadata(&resolved.path)
        .map(|m| m.is_dir)
        .map_err(|e| Some(format!("{href}: {e}")))?;
    Ok(Row {
        vfs: std::sync::Arc::clone(&root.vfs),
        name: resolved.path.segments().last().cloned().unwrap_or_default(),
        path: resolved.path,
        is_dir,
    })
}

/// A native question with a button that goes ahead and Cancel. True is yes.
fn ask(app: &AppHandle, title: &str, message: &str, ok: &str) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    #[cfg_attr(mobile, allow(unused_mut))]
    let mut ask = app
        .dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(ok.into(), "Cancel".into()));
    #[cfg(desktop)]
    if let Some(win) = app.get_webview_window(WINDOW) {
        ask = ask.parent(&win);
    }
    ask.blocking_show()
}

/// `/.ts/act`: one of the backend's own verbs on a row (`Vfs::row_info`) —
/// asked about first where the verb says so, carried out by `Vfs::act` with the
/// status line saying how far it has got, and the page drawn again.
///
/// The verb is looked up again here rather than trusted from the link: the
/// question it asks and whether it is on offer at all are the backend's answer
/// *now*, not when the page was drawn.
pub(crate) fn act(app: &AppHandle, url: &tauri::Url) {
    let app = app.clone();
    let url = url.clone();
    thread::spawn(move || {
        let r = match row(&app, &url) {
            Ok(r) => r,
            Err(Some(why)) => return fail(&app, &why, false),
            Err(None) => return,
        };
        let id = param(&url, "a").unwrap_or_default();
        let Some(action) = r
            .vfs
            .row_info(&r.path)
            .and_then(|i| i.actions.into_iter().find(|a| a.id == id))
        else {
            return fail(&app, &format!("That cannot be done to {} now.", r.name), false);
        };
        if let Some(c) = &action.confirm
            && !ask(&app, &action.label, &c.message, &c.ok)
        {
            return;
        }
        let Some(_turn) = Turn::take() else {
            return fail(&app, BUSY_SAYS, false);
        };
        let what = format!("{} · {}", action.label, r.name);
        let mut last = Instant::now() - TICK;
        let done = r.vfs.act(&r.path, &id, &mut |done, total| {
            if last.elapsed() < TICK {
                return;
            }
            last = Instant::now();
            let line = match total {
                Some(t) if t > 0 => (format!("{what} · {} of {} MB", mb(done), mb(t)), done as f64 / t as f64),
                _ => (format!("{what} · {} MB", mb(done)), 0.0),
            };
            progress(&app, Some((&line.0, line.1)));
        });
        progress(&app, None);
        match done {
            Ok(said) => {
                eval(&app, "location.reload()");
                if let Some(said) = said {
                    notify(&app, &said);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => fail(&app, &format!("{}: {}: {e}", action.label, r.name), false),
        }
    });
}

/// `/.ts/remove`: **Delete**, from a row's strip — a file, or a folder with
/// nothing in it. Asked every time; never recursive. The backend may add a
/// line to the question (`Vfs::remove_note`).
pub(crate) fn remove(app: &AppHandle, url: &tauri::Url) {
    let app = app.clone();
    let url = url.clone();
    thread::spawn(move || {
        let r = match row(&app, &url) {
            Ok(r) => r,
            Err(Some(why)) => return fail(&app, &why, false),
            Err(None) => return,
        };
        if !r.vfs.writable() || r.path.segments().is_empty() {
            return fail(&app, &format!("{} cannot be deleted here.", r.name), false);
        }
        if r.is_dir && r.vfs.read_dir(&r.path).is_ok_and(|v| !v.is_empty()) {
            return fail(&app, &format!("{} is not empty; delete what is in it first.", r.name), false);
        }
        let place = app
            .try_state::<Serving>()
            .and_then(|s| s.state().cfg.root_name())
            .map(|n| format!(" from {n}"))
            .unwrap_or_default();
        let head = match r.is_dir {
            true => format!("Delete the empty folder {}{place}?", r.name),
            false => format!("Delete {}{place}?", r.name),
        };
        let mut message = format!("{head}\n\nIt cannot be brought back from here.");
        if let Some(note) = r.vfs.remove_note(&r.path) {
            message.push_str(&format!("\n\n{note}"));
        }
        if !ask(&app, "Delete", &message, "Delete") {
            return;
        }
        match r.vfs.remove(&r.path) {
            Ok(()) => eval(&app, "location.reload()"),
            Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty => {
                fail(&app, &format!("{} is not empty; delete what is in it first.", r.name), false)
            }
            Err(e) => fail(&app, &format!("Could not delete {}: {e}", r.name), false),
        }
    });
}
