//! One file alone in the main window: a file another app handed over
//! (`treeserve::Lone`). Telechore's `docs/tasks/tree-browser.md`, *Opening a
//! file from another app*, has the design; `docs/mockups/open-from-apps.html`
//! the sketch.
//!
//! The window's root is the file's folder and the page is drawn as a document
//! window draws its file — no tree, no Up, Start in Up's place — with the way
//! on at the left end: **Save to Files** for a copy the app made only to show
//! the file (a phone), **Show folder** for the file where it is (a desktop).

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::{
    begin_open, committed_in, fail, note_in, note_landing, off_the_callback, on_the_start_page, open_superseded,
    push_page, remember_root_id, replace_page, replace_page_in, show, Serving, WINDOW,
};

/// The main window is being given a file handed over on a desktop: from the
/// moment it is promised one until its root is set, it counts as open, so a
/// second file arriving meanwhile — the rest of a Linux `%F` or a macOS
/// `Opened` set, or a quick second Open with — gets a window of its own
/// instead of cancelling the first.
///
/// The file it was claimed for, so that the same file again brings the main
/// window forward rather than opening a second window beside it.
#[cfg(desktop)]
static MAIN_CLAIMED: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Run once the page is on screen, for what can only be done to the page that
/// is: the history behind it, a note in it.
pub type Landed = Box<dyn FnOnce(&AppHandle) + Send + 'static>;

/// How long the passing note stays. As long as the full-screen controls do.
#[cfg_attr(desktop, allow(dead_code))]
const NOTE_MS: u64 = 2500;

/// Shows `file` alone in the main window. `temporary`: a copy made only to show
/// it, offered Save to Files and deleted when the reader leaves it by Start.
/// Not added to Recent: a file is not a root anyone chose. `dated`: its time
/// is its own, not only when a copy was made. `landed` runs once the file's
/// page is on screen — on Android, where the history behind it is cleared so
/// that Back goes back to the app that sent it.
pub fn open_lone(app: &AppHandle, file: PathBuf, temporary: bool, dated: bool, landed: Option<Landed>) {
    let app = app.clone();
    let open_id = begin_open();
    thread::spawn(move || {
        let resolved = file.canonicalize().ok().filter(|f| f.is_file());
        let back = app.clone();
        let _ = app.run_on_main_thread(move || {
            if open_superseded(open_id) {
                #[cfg(desktop)]
                release_main();
                return;
            }
            match resolved {
                Some(file) => {
                    let served = serve_lone(&back, &file, temporary, dated);
                    // The root is set: from here the main window is open to
                    // anything that asks, and the claim has done its work.
                    #[cfg(desktop)]
                    release_main();
                    if let Some(url) = served {
                        if let Some(landed) = landed {
                            when_landed(&back, WINDOW, url, landed);
                        }
                    }
                }
                None => {
                    #[cfg(desktop)]
                    release_main();
                    fail(
                        &back,
                        &format!("{} could not be opened.", treeserve::util::display_path(&file)),
                        false,
                    )
                }
            }
        });
    });
}

/// A file the system handed to the app on a desktop — Open with, or named on
/// the command line — shown where it is. The main window takes it when
/// nothing is open there, which is how a launch for a file starts; otherwise
/// a document window does, and one already showing it comes to the front.
/// Linux and Windows reach this from the command line and the single-instance
/// hook, macOS from `RunEvent::Opened`: the one rule for all three.
#[cfg(desktop)]
pub(crate) fn open_handed(app: &AppHandle, file: PathBuf) {
    let Ok(file) = file.canonicalize() else {
        return fail(app, &format!("{} could not be opened.", treeserve::util::display_path(&file)), false);
    };
    let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
        return;
    };
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    let cfg = &serving.state().cfg;
    let document = treeserve::util::href_path(&[name.to_string_lossy().into_owned()]);
    let id = treeserve::util::display_path(dir);
    let front = |label: &str| {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.unminimize();
            let _ = win.show();
            let _ = win.set_focus();
        }
    };
    {
        let mut claimed = MAIN_CLAIMED.lock().unwrap_or_else(|e| e.into_inner());
        match claimed.as_ref() {
            // On its way to the main window already.
            Some(f) if *f == file => return front(WINDOW),
            Some(_) => {}
            None if cfg.root().is_none() => {
                *claimed = Some(file.clone());
                drop(claimed);
                open_lone(app, file, false, true, None);
                return front(WINDOW);
            }
            None => {}
        }
    }
    if cfg.lone().is_some_and(|l| l.document == document) && cfg.root().is_some_and(|r| r.id == id) {
        return front(WINDOW);
    }
    if let Some(label) = cfg.window_showing(&id, &document) {
        return front(&label);
    }
    super::own_window(app, treeserve::Root::local(dir.to_path_buf()), document, true);
}

/// Lets [`MAIN_CLAIMED`] go: called once the main window's root is set, or
/// once the open it was claimed for has come to nothing.
#[cfg(desktop)]
fn release_main() {
    *MAIN_CLAIMED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Re-roots the main window at `file`'s folder, for that file alone, and puts
/// its page on screen: a step from the start page, a change of scene from
/// anything else. The page's address, or `None` where nothing was served.
fn serve_lone(app: &AppHandle, file: &Path, temporary: bool, dated: bool) -> Option<String> {
    let serving = app.try_state::<Serving>()?;
    let dir = file.parent()?;
    let name = file.file_name()?.to_string_lossy().into_owned();
    let document = treeserve::util::href_path(&[name.clone()]);
    let cfg = &serving.state().cfg;
    cfg.set_root(dir.to_path_buf());
    // The folder of a copy is the app's own, and its name nobody's business;
    // a file in its own folder is that folder's, as any root is.
    if temporary {
        cfg.set_root_name(Some(name));
    }
    cfg.set_lone(Some(treeserve::Lone { document: document.clone(), temporary, dated }));
    let url = format!("{}{}", serving.entry.trim_end_matches('/'), document);
    note_landing(&url);
    match on_the_start_page(app) {
        true => push_page(app, &url),
        false => replace_page(app, &url),
    }
    if let Some(win) = app.get_webview_window(WINDOW) {
        show(&win);
    }
    Some(url)
}

/// The file the main window is there for, on this device's disk.
fn lone_file(app: &AppHandle) -> Option<(PathBuf, treeserve::Lone)> {
    let serving = app.try_state::<Serving>()?;
    let cfg = &serving.state().cfg;
    let lone = cfg.lone()?;
    let root = cfg.root()?;
    let name = treeserve::util::percent_decode(lone.document.trim_start_matches('/'));
    Some((PathBuf::from(&root.id).join(name), lone))
}

/// Start, on a copy made only to show it: the copy goes with the page, so that
/// Back has nothing to come back to (the page itself is replaced, not left
/// behind). Called before the root is closed.
pub(crate) fn leaving(app: &AppHandle) {
    if let Some((file, lone)) = lone_file(app)
        && lone.temporary
    {
        let _ = std::fs::remove_file(&file);
        // Its folder was its own (`Handed.kt`), and is empty now.
        if let Some(dir) = file.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Show folder: the file stays on screen, now with its folder's tree. In the
/// main window the folder becomes a root like any other, in Recent; in a
/// document window it becomes a folder window of the same folder, its root
/// unchanged.
pub(crate) fn show_folder(app: &AppHandle, label: &str) {
    let Some(serving) = app.try_state::<Serving>() else {
        return;
    };
    let cfg = &serving.state().cfg;
    let document = match label == WINDOW {
        true => {
            let Some(lone) = cfg.lone().filter(|l| !l.temporary) else {
                return;
            };
            let Some(root) = cfg.root() else {
                return;
            };
            cfg.set_lone(None);
            let id = root.id.clone();
            off_the_callback(app, move |app| remember_root_id(app, &id));
            lone.document
        }
        false => {
            let Some(document) = cfg.window(label).and_then(|w| w.document) else {
                return;
            };
            cfg.window_shows_folder(label);
            document
        }
    };
    let url = format!("{}{}", serving.entry.trim_end_matches('/'), document);
    replace_page_in(app, label, &url);
}

/// Save to Files, on a copy made only to show it: kept in Files, and Files'
/// own page of it on screen in place of this one.
#[cfg(mobile)]
pub(crate) fn save(app: &AppHandle) {
    let Some((file, lone)) = lone_file(app) else {
        return;
    };
    if !lone.temporary {
        return;
    }
    save_and_show(app, file, None);
}

/// Keeps `file` in Files and shows it there, the root Files' own: what Save to
/// Files does, and what a file handed over to be saved goes straight to.
/// A passing note says so once the page is there, and `landed` runs then too.
#[cfg(mobile)]
pub fn save_and_show(app: &AppHandle, file: PathBuf, landed: Option<Landed>) {
    let app = app.clone();
    let open_id = begin_open();
    thread::spawn(move || {
        // Its real path, as `open_root` serves a folder by: on Android the
        // app's folder is reached through a link, and a root that is not its
        // real path refuses every file in it as outside it.
        let saved = super::app_storage_dir(&app)
            .ok_or_else(|| std::io::Error::other("this device gave the app no storage"))
            .and_then(|files| files.canonicalize())
            .and_then(|files| keep_in(&files, &file));
        // The copy's folder was its own (`Handed.kt`), and is empty now that the
        // copy has moved, or been found already there.
        if saved.is_ok()
            && let Some(dir) = file.parent()
        {
            let _ = std::fs::remove_dir(dir);
        }
        let back = app.clone();
        let _ = app.run_on_main_thread(move || {
            if open_superseded(open_id) {
                return;
            }
            let dest = match saved {
                Ok(dest) => dest,
                Err(e) => return fail(&back, &format!("The file could not be saved to Files: {e}"), false),
            };
            let (Some(dir), Some(name)) = (dest.parent(), dest.file_name()) else {
                return;
            };
            let Some(serving) = back.try_state::<Serving>() else {
                return;
            };
            let cfg = &serving.state().cfg;
            let id = treeserve::util::display_path(dir);
            cfg.set_root(dir.to_path_buf());
            if let Some((label, _)) = cfg.places.iter().find(|(_, p)| *p == id) {
                cfg.set_root_name(Some(label.clone()));
            }
            let url = format!(
                "{}{}",
                serving.entry.trim_end_matches('/'),
                treeserve::util::href_path(&[name.to_string_lossy().into_owned()])
            );
            note_landing(&url);
            replace_page(&back, &url);
            when_landed(
                &back,
                WINDOW,
                url,
                Box::new(move |app| {
                    pass_note(app, WINDOW, "Saved to Files");
                    if let Some(landed) = landed {
                        landed(app);
                    }
                }),
            );
        });
    });
}

/// `file` moved into `files` under its own name. The same name with the same
/// bytes is the copy already there, which is reused; the same name with other
/// bytes is `name (2).ext`, and so on.
#[cfg_attr(desktop, allow(dead_code))]
pub(crate) fn keep_in(files: &Path, file: &Path) -> std::io::Result<PathBuf> {
    let name = file
        .file_name()
        .ok_or_else(|| std::io::Error::other("the file has no name"))?
        .to_string_lossy()
        .into_owned();
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    for n in 1u32.. {
        let dest = match n {
            1 => files.join(&name),
            n => files.join(format!("{stem} ({n}){ext}")),
        };
        if !dest.exists() {
            if std::fs::rename(file, &dest).is_err() {
                std::fs::copy(file, &dest)?;
                let _ = std::fs::remove_file(file);
            }
            return Ok(dest);
        }
        if same_bytes(&dest, file) {
            let _ = std::fs::remove_file(file);
            return Ok(dest);
        }
        if n > 999 {
            break;
        }
    }
    Err(std::io::Error::other("Files has too many copies by that name"))
}

/// Whether two files hold the same bytes: the sizes first, then the bytes, a
/// block at a time — a large PDF saved twice is not held twice in memory.
#[cfg_attr(desktop, allow(dead_code))]
fn same_bytes(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) if x.len() == y.len() => {}
        _ => return false,
    }
    let (Ok(mut x), Ok(mut y)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    let (mut bx, mut by) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = match x.read(&mut bx) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            return y.read(&mut by[..1]).is_ok_and(|m| m == 0);
        }
        if y.read_exact(&mut by[..n]).is_err() || bx[..n] != by[..n] {
            return false;
        }
    }
}

/// A note in the status line of the window `label` that goes by itself.
#[cfg_attr(desktop, allow(dead_code))]
pub(crate) fn pass_note(app: &AppHandle, label: &str, text: &str) {
    note_in(app, label, Some(text));
    let app = app.clone();
    let label = label.to_string();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(NOTE_MS));
        note_in(&app, &label, None);
    });
}

/// Runs `f` once the window `label` has committed the page at `url`, from a
/// thread of its own; gives up after ten seconds, a page that never came.
fn when_landed(app: &AppHandle, label: &str, url: String, f: Landed) {
    let app = app.clone();
    let label = label.to_string();
    thread::spawn(move || {
        let want = url.parse::<tauri::Url>().ok().map(|u| u.path().to_string());
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            thread::sleep(Duration::from_millis(100));
            let here = app.get_webview_window(&label).and_then(|w| w.url().ok()).map(|u| u.path().to_string());
            if committed_in(&app, &label) && here.is_some() && here == want {
                return f(&app);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::keep_in;

    /// Files keeps one copy of the same file under its name, and a second
    /// file of that name beside it, numbered.
    #[test]
    fn files_keeps_a_name_once_and_numbers_the_rest() {
        let base = std::env::temp_dir().join(format!("ts-lone-keep-{}", std::process::id()));
        let (files, handed) = (base.join("files"), base.join("handed"));
        std::fs::create_dir_all(&files).unwrap();
        std::fs::create_dir_all(&handed).unwrap();
        let hand = |bytes: &str| {
            let f = handed.join("report.pdf");
            std::fs::write(&f, bytes).unwrap();
            f
        };

        assert_eq!(keep_in(&files, &hand("one")).unwrap(), files.join("report.pdf"));
        // The same bytes again: the copy already there, and nothing new.
        assert_eq!(keep_in(&files, &hand("one")).unwrap(), files.join("report.pdf"));
        assert!(!handed.join("report.pdf").exists(), "the copy given is used up");
        // Other bytes under the name: numbered beside it.
        assert_eq!(keep_in(&files, &hand("two")).unwrap(), files.join("report (2).pdf"));
        assert_eq!(keep_in(&files, &hand("three")).unwrap(), files.join("report (3).pdf"));
        assert_eq!(keep_in(&files, &hand("two")).unwrap(), files.join("report (2).pdf"));
        assert_eq!(std::fs::read_to_string(files.join("report (3).pdf")).unwrap(), "three");

        // Past one block, compared a block at a time: the same bytes are the
        // copy already there, one byte changed at the end is another file.
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let hand_big = |bytes: &[u8]| {
            let f = handed.join("big.bin");
            std::fs::write(&f, bytes).unwrap();
            f
        };
        assert_eq!(keep_in(&files, &hand_big(&big)).unwrap(), files.join("big.bin"));
        assert_eq!(keep_in(&files, &hand_big(&big)).unwrap(), files.join("big.bin"));
        let mut other = big.clone();
        *other.last_mut().unwrap() ^= 1;
        assert_eq!(keep_in(&files, &hand_big(&other)).unwrap(), files.join("big (2).bin"));

        std::fs::remove_dir_all(&base).unwrap();
    }
}
