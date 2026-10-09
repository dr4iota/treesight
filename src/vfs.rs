//! The filesystem seam: what the renderer asks of a served tree.
//!
//! Every page is rendered from the answers to five questions — resolve,
//! metadata, list, read, open — and nothing else. [`LocalFs`] answers them
//! with `std::fs`, exactly as the call sites used to inline; an embedder can
//! swap in a backend that answers them from somewhere else entirely and every
//! view works unchanged. The server itself never learns where the bytes are.

use std::fs;
use std::io::{self, Read, Seek};
use std::path::{Component, PathBuf};
use std::time::SystemTime;

use crate::util::display_path;

/// A path inside a served root: percent-decoded segments with no separators
/// and no `.` or `..` — the URL side of the server already speaks exactly
/// this. Spelled out it is always `/`-joined, whatever the backend's host
/// uses, which is what keeps a served page identical over every backend.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct VfsPath(Vec<String>);

impl VfsPath {
    /// The served root itself.
    pub fn root() -> VfsPath {
        VfsPath(Vec::new())
    }

    pub fn new(segments: Vec<String>) -> VfsPath {
        VfsPath(segments)
    }

    pub fn segments(&self) -> &[String] {
        &self.0
    }

    pub fn join(&self, name: &str) -> VfsPath {
        let mut segs = self.0.clone();
        segs.push(name.to_string());
        VfsPath(segs)
    }
}

/// What a path turned out to be. `mtime` and `len` also make the ETag, so a
/// backend that cannot answer `mtime` serves without conditional requests
/// rather than with wrong ones.
pub struct Meta {
    pub is_dir: bool,
    pub is_file: bool,
    pub len: u64,
    pub mtime: Option<SystemTime>,
    /// Unix permission bits, when the backend knows them — what Save As
    /// restores on a copy, so a downloaded script stays executable.
    pub mode: Option<u32>,
}

/// One directory entry, as a listing shows it. `is_dir` follows symlinks —
/// a link to a directory lists and walks as a directory — while `size` and
/// `mtime` describe the entry itself, which is what `std::fs::DirEntry`
/// answers without a second syscall per row.
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<SystemTime>,
}

/// What `serve_raw` streams from: sequential reads plus the one seek a Range
/// request needs.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// Why a backend would not answer for a path.
///
/// Named for [`Vfs::resolve`], which is where it started, but it is the seam's
/// whole vocabulary for a refusal now — [`ResolveError::of`] classifies the
/// `io::Error`s the other four methods return, so every call in a request
/// sorts a failure the same way and the page can say which of these it hit.
///
/// **Two of these used to be one.** The enum had `Missing` and `Outside` and
/// nothing else, so a backend with no answer had to spell every failure as
/// "nothing there": a share whose host had gone gave a 404, and so did a folder
/// whose permissions merely exclude you. Over a remote root that is the
/// difference between *go and look at the network* and *you cannot read this*,
/// and the reader was told neither. The words are the three a shortcut row
/// already draws, deliberately — see `RootStatus`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResolveError {
    /// Nothing there.
    Missing,
    /// There, but it leads out of the served root — a symlink whose target is
    /// outside. Refused for the same reason `..` is refused in a URL.
    Outside,
    /// It answered, and would not have us: a folder whose permissions exclude
    /// us, a grant the platform has revoked.
    Denied,
    /// It did not answer at all: a session that has gone, a timeout, a mapped
    /// drive whose host is off.
    Unreachable,
}

impl ResolveError {
    /// A backend's `io::Error`, as the seam's word for it.
    ///
    /// The one classifier, so `resolve` and every call after it agree. Only the
    /// two kinds that mean something specific; everything else is a backend
    /// that did not answer, which is the honest reading of an error nobody can
    /// name — and the reading that sends a reader to the machine rather than to
    /// a file that is sitting right there.
    pub fn of(e: &io::Error) -> ResolveError {
        match e.kind() {
            io::ErrorKind::NotFound => ResolveError::Missing,
            io::ErrorKind::PermissionDenied => ResolveError::Denied,
            _ => ResolveError::Unreachable,
        }
    }

    /// The HTTP status a page for this refusal is served with, and the words on
    /// it. *Forbidden* is [`Self::Outside`]'s and stays its own: that is this
    /// server refusing, where `Denied` is the far end refusing.
    pub fn as_reply(self) -> (u16, &'static str) {
        match self {
            ResolveError::Missing => (404, "not found"),
            ResolveError::Outside => (403, "forbidden"),
            ResolveError::Denied => (403, "not readable"),
            // The backend is a gateway as far as this server is concerned, and
            // it did not answer. 502 is that, and it keeps 404 meaning the one
            // thing a reader can act on by looking somewhere else.
            ResolveError::Unreachable => (502, "not answering"),
        }
    }
}

/// The reader's **Cancel** for the transfer under way, where a backend can
/// see it.
///
/// The shell sets it from the status line and clears it when the next
/// transfer starts; the copy loop looks at it between chunks. A backend that
/// waits inside one chunk — for a dropped connection to come back, say —
/// looks too, or Cancel would do nothing until that wait ran out.
pub mod cancel {
    use std::sync::atomic::{AtomicBool, Ordering};

    static ASKED: AtomicBool = AtomicBool::new(false);

    pub fn ask() {
        ASKED.store(true, Ordering::SeqCst);
    }

    pub fn clear() {
        ASKED.store(false, Ordering::SeqCst);
    }

    pub fn asked() -> bool {
        ASKED.load(Ordering::SeqCst)
    }
}

/// A file being written through [`Vfs::create_file`].
///
/// `finish` and not `Drop`, because closing is where a remote write reports
/// that it failed — a full disk, a dropped connection — and a drop has nowhere
/// to say so.
///
/// **A write that does not finish leaves the folder as it was.** Dropped
/// without `finish`, or with a `finish` that fails, the backend takes away
/// whatever it made — the new file, or the temporary one a replacement was
/// being written into — and a file being replaced is still the old one. That
/// is the backend's to do and not the caller's, because only the backend knows
/// what it made: a caller removing "the file" after a failed replacement
/// removed the one it was asked to keep.
///
/// `Ok(Some(note))` is a write that finished with something the reader should
/// be told — the new file is in place, but the one it replaced could not be
/// cleared away and is still there under another name, say. Not a failure,
/// and not to be kept quiet either.
pub trait WriteFile: io::Write + Send {
    fn finish(self: Box<Self>) -> io::Result<Option<String>>;
}

/// What a backend can say about one row of a listing beyond its name, size
/// and time — asked through [`Vfs::row_info`]: a line of its own words, verbs
/// of its own on the row. The page draws all of it in the row's strip, under
/// ⋯, and gives none of it a meaning: what the words say and what the verbs do
/// are the backend's.
#[derive(Clone, Debug, Default)]
pub struct RowInfo {
    /// The strip's first line, in the backend's words.
    pub line: Option<RowLine>,
    /// Short words that need the reader's attention, drawn as warning pills
    /// after the line.
    pub warn: Vec<String>,
    /// A filled mark after the row's name, with this title. The one state of a
    /// row worth showing before ⋯ is opened.
    pub mark: Option<String>,
    /// The backend's own verbs, in order. Download and Delete are the page's
    /// and come after them.
    pub actions: Vec<RowAction>,
    /// False draws the row greyed and without a link: there is nothing behind
    /// it the backend can serve now.
    pub unreachable: bool,
}

/// The first line of a row's strip, in two lengths: the page shows one by its
/// width, and on a narrow page puts the row's modified time before it, since
/// the narrow table has no column for that. Plain text; the page escapes it.
#[derive(Clone, Debug, Default)]
pub struct RowLine {
    pub wide: String,
    pub narrow: String,
}

/// One of a backend's verbs on a row, carried out by [`Vfs::act`] when the
/// shell is handed its link.
#[derive(Clone, Debug)]
pub struct RowAction {
    /// What [`Vfs::act`] is told. Letters, digits and `-` only: it rides in a
    /// link.
    pub id: String,
    /// SVG path data, as [`crate::HeaderFlag::icon`] is.
    pub icon: String,
    pub label: String,
    pub title: String,
    /// A toggle that is on, drawn lit.
    pub on: bool,
    /// Asked before it is done, in a native box.
    pub confirm: Option<Confirm>,
    /// Also a pill on the file's own page, after Download.
    pub on_file_page: bool,
}

/// A question asked before a verb is carried out.
#[derive(Clone, Debug)]
pub struct Confirm {
    pub message: String,
    /// The button that goes ahead.
    pub ok: String,
}

/// A line under the header of a page — a folder or a file — that says what
/// the page is when it is not what it seems: a copy, served while the place it
/// came from does not answer.
#[derive(Clone, Debug)]
pub struct Notice {
    pub warn: bool,
    /// Plain text; the page escapes it.
    pub text: String,
    /// A link at the line's end: (words, href).
    pub link: Option<(String, String)>,
}

/// `name` as the name of a new file or folder, or why it cannot be one.
///
/// One rule for every backend, so a name refused over SSH is refused on a
/// local disk too: not empty once trimmed, no separator of either kind, not
/// `.` or `..`, no NUL, and at most 255 bytes — the limit almost every
/// filesystem shares. Leading and trailing spaces are trimmed rather than
/// kept: a name that differs from another only by a space at its end is a
/// trap on every system that shows it.
pub fn new_name(name: &str) -> Result<&str, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A name is needed.");
    }
    if name == "." || name == ".." {
        return Err("That name is taken by the folder system itself.");
    }
    if name.contains(['/', '\\', '\0']) {
        return Err("A name cannot hold / or \\.");
    }
    if name.len() > 255 {
        return Err("That name is too long.");
    }
    Ok(name)
}

/// A served tree. Paths are [`VfsPath`]s relative to the backend's root.
///
/// Confinement — refusing a path that leads out of the root — is
/// [`resolve`](Vfs::resolve)'s job, and only its job: it runs once per
/// request, on the URL's path. The other methods are asked either about the
/// path `resolve` returned or about children of it made by joining names
/// [`read_dir`](Vfs::read_dir) reported — the README preview, the tree walk,
/// search descent — and those joins are answered as the host answers them,
/// symlinks followed, exactly as the inlined `fs` calls always did. So the
/// escape check guards what a URL can *navigate to*, not every byte a listing
/// may summarize; a backend is free to confine harder in every method, and
/// the renderer depends on it either way only through `resolve`.
pub trait Vfs: Send + Sync {
    /// The canonical form of `path`: symlinks resolved, confinement checked.
    fn resolve(&self, path: &VfsPath) -> Result<VfsPath, ResolveError>;

    fn metadata(&self, path: &VfsPath) -> io::Result<Meta>;

    /// Entries as the backend finds them: unfiltered and unsorted. Hiding
    /// dotfiles and ordering rows are page policy, not filesystem truth, so
    /// they stay with the page.
    fn read_dir(&self, path: &VfsPath) -> io::Result<Vec<Entry>>;

    /// The whole file. Callers cap what they ask for (`MAX_HIGHLIGHT_BYTES`);
    /// anything unbounded streams through [`open`](Vfs::open) instead.
    fn read(&self, path: &VfsPath) -> io::Result<Vec<u8>>;

    fn open(&self, path: &VfsPath) -> io::Result<Box<dyn ReadSeek>>;

    /// Whether a copy of a file here is something to offer.
    ///
    /// Always the same for a whole tree, which is why it is asked of the backend
    /// and not of the file: what varies is where the tree *is*. A copy of a file
    /// on a machine you are logged into is a copy you did not have; a copy of one
    /// already on this device is the same bytes under a second name, and on a
    /// platform with nowhere to put it a Download button is a button that cannot
    /// do the thing it says.
    fn downloadable(&self) -> bool {
        true
    }

    /// Whether this tree takes new files and folders — **Upload** and **New
    /// folder** in a folder's header. Off unless a backend says otherwise, and
    /// for the whole tree, as [`Self::downloadable`] is.
    ///
    /// The other side of the seam from everything above: those questions only
    /// read. A backend that answers yes implements the three calls below, and
    /// nothing else here ever writes — the server has no route that does; the
    /// shell, which the reader clicked, is the one caller.
    fn writable(&self) -> bool {
        false
    }

    /// Whether `path` is itself a symbolic link, not following it. Asked
    /// before a write offers to replace a name: `metadata` follows the link,
    /// so a link to a file looked like a file to replace, and was refused
    /// only after the reader had said Replace. False where a backend has no
    /// links, or cannot tell.
    fn is_link(&self, _path: &VfsPath) -> bool {
        false
    }

    /// A folder called `name` inside `dir`. `AlreadyExists` when anything of
    /// that name is there — never a folder merged into an existing one.
    /// `name` has been through [`new_name`].
    fn create_dir(&self, _dir: &VfsPath, _name: &str) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// A file called `name` inside `dir`, to write into. With `replace` false
    /// it is `AlreadyExists` when anything of that name is there, checked by
    /// the backend at the moment it creates the file — a name that was free
    /// when the reader was asked and is taken by the time the bytes arrive is
    /// refused, not overwritten. With `replace` true a file there is replaced
    /// **when the write finishes**, and not before: until [`WriteFile::finish`]
    /// succeeds it is the file it was. A folder or a symbolic link of that name
    /// is refused either way — replacing a link would write wherever it points.
    /// `name` has been through [`new_name`].
    fn create_file(
        &self,
        _dir: &VfsPath,
        _name: &str,
        _replace: bool,
    ) -> io::Result<Box<dyn WriteFile>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Whether a document from here came off the machine this is running on.
    ///
    /// Not a claim about the *bytes* — a cloned repository is somebody else's
    /// text on your own disk — but about the transport, which is the line a
    /// browser draws too: a file you can already open with any other program on
    /// this device, versus one a remote machine just handed us.
    ///
    /// What turns on it is whether a document's own raw HTML is drawn as markup
    /// (`md::RawHtml`). Script is not the reason: no element runs under the
    /// pages' CSP, wherever the document came from. Style is. `style-src
    /// 'unsafe-inline'` is what lets our own pages carry their theme, and it is
    /// also enough for a remote document to paint a convincing copy of the
    /// password box this app puts on the screen — over the very page that was
    /// waiting for a connection to that machine.
    ///
    /// `false` is the default, so a backend that says nothing is treated as
    /// somewhere else. A new remote is then safe before anybody remembers it
    /// exists, and the two backends that *are* this device say so below.
    fn on_this_device(&self) -> bool {
        false
    }

    /// The most this backend will hand over in one piece, where it has a limit
    /// at all. `None` — the answer for anything reading a filesystem or a
    /// stream it can seek — means whatever it can reach, it can serve.
    ///
    /// A backend that copies a whole file to answer [`Self::open`] has one, and
    /// the page needs it *before* it draws: every branch that shows a picture, a
    /// video or a PDF points an element at the bytes, and an element pointed at
    /// a refusal is a broken box with nothing to read in it. Past this, the page
    /// says so in words instead.
    fn open_limit(&self) -> Option<u64> {
        None
    }

    /// What to draw in the strip under `path`'s row, beyond the page's own
    /// Download and Delete. `None` — the default — is a backend with nothing to
    /// add.
    fn row_info(&self, _path: &VfsPath) -> Option<RowInfo> {
        None
    }

    /// [`Self::row_info`] for every row of one listing, asked once per page so a
    /// backend can answer a whole folder from one look. `entries` are the
    /// rows' names under `dir`, in the order the answer comes back in.
    fn row_infos(&self, dir: &VfsPath, entries: &[Entry]) -> Vec<Option<RowInfo>> {
        entries.iter().map(|e| self.row_info(&dir.join(&e.name))).collect()
    }

    /// A line under the header of `path`'s page. See [`Notice`].
    fn notice(&self, _path: &VfsPath) -> Option<Notice> {
        None
    }

    /// Carries out the verb `id` of one of [`Self::row_info`]'s actions on
    /// `path`. Called by the shell, off the navigation callback and after any
    /// [`Confirm`] was answered yes; `progress` is told bytes done and the
    /// whole where the verb moves any. `Ok(Some(words))` is something to tell
    /// the reader.
    fn act(
        &self,
        _path: &VfsPath,
        _id: &str,
        _progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> io::Result<Option<String>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Deletes the file at `path`, or the folder there if it is empty —
    /// `DirectoryNotEmpty` if it is not; nothing is ever deleted recursively.
    /// Offered where [`Self::writable`] is.
    fn remove(&self, _path: &VfsPath) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// Whether **Delete** is offered on this tree's rows: by default where the
    /// tree takes writes, and also where it only lets things go — a desktop's
    /// own folders, whose files the shell moves to the Trash.
    fn deletable(&self) -> bool {
        self.writable()
    }

    /// Where `path` is on this machine's disk, for a backend that is this
    /// machine's disk. The shell moves a deleted file there to the system's
    /// Trash rather than asking [`Self::remove`], which is permanent.
    fn local_path(&self, _path: &VfsPath) -> Option<PathBuf> {
        None
    }

    /// A last line for the question Delete asks about `path` — what the
    /// backend keeps of it after, say.
    fn remove_note(&self, _path: &VfsPath) -> Option<String> {
        None
    }

    /// The RootId that would serve `path` as a root of its own — what the
    /// tree's per-directory re-root link carries. For a local root that is
    /// the display-form host path, which is also what a RootId *is* for a
    /// local root: the bare path, exactly the string `recent.txt` has always
    /// held. Remote backends prefix a scheme (`ssh:<bookmark>:/path`); a
    /// single-letter prefix is a Windows drive, not a scheme.
    fn root_id_at(&self, path: &VfsPath) -> String;

    /// The RootId of the served root itself.
    fn root_id(&self) -> String {
        self.root_id_at(&VfsPath::root())
    }
}

/// The local filesystem, rooted at a canonicalized directory.
///
/// The stored root keeps the verbatim form `canonicalize` gave it (on Windows
/// that is `\\?\…`), because [`resolve`](Vfs::resolve) compares freshly
/// canonicalized paths against it and the two spellings would never match.
/// Display strings strip it via [`display_path`].
pub struct LocalFs {
    root: PathBuf,
}

impl LocalFs {
    /// `root` should already be canonicalized, as [`crate::Config::new`]'s
    /// callers have always done.
    pub fn new(root: PathBuf) -> LocalFs {
        LocalFs { root }
    }

    /// Downloading a local file means saving a copy of it somewhere else, which
    /// is a thing a desktop can do and Android cannot: the only local roots
    /// there are the app's own directories, and "save a copy" would mean copying
    /// a file the reader already has into a folder they are already looking at.
    /// A granted folder is not this backend at all.
    fn copies_are_worth_making() -> bool {
        !cfg!(target_os = "android")
    }

    fn host(&self, path: &VfsPath) -> PathBuf {
        let mut abs = self.root.clone();
        for seg in path.segments() {
            abs.push(seg);
        }
        abs
    }
}

impl Vfs for LocalFs {
    /// This machine's own filesystem, by definition.
    fn on_this_device(&self) -> bool {
        true
    }

    /// On a desktop, where Delete goes to the Trash. Not on a phone: the only
    /// local roots there are the app's own folders, and a granted folder is
    /// not this backend.
    fn deletable(&self) -> bool {
        Self::copies_are_worth_making()
    }

    /// The display form: Windows' `\\?\` prefix off, which is how every
    /// other program, the Trash included, names the file.
    fn local_path(&self, path: &VfsPath) -> Option<PathBuf> {
        Some(PathBuf::from(display_path(&self.host(path))))
    }

    /// By `symlink_metadata`, which does not follow the link it is asked about.
    fn is_link(&self, path: &VfsPath) -> bool {
        fs::symlink_metadata(self.host(path)).is_ok_and(|m| m.file_type().is_symlink())
    }

    /// Permanent: a file, or a folder with nothing in it. What the shell falls
    /// back to when the Trash will not take something, after asking.
    fn remove(&self, path: &VfsPath) -> io::Result<()> {
        if path.segments().is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the served root is not deleted from here"));
        }
        let host = self.host(path);
        match fs::symlink_metadata(&host)?.is_dir() {
            true => fs::remove_dir(&host),
            false => fs::remove_file(&host),
        }
    }

    fn downloadable(&self) -> bool {
        Self::copies_are_worth_making()
    }

    fn resolve(&self, path: &VfsPath) -> Result<VfsPath, ResolveError> {
        // canonicalize resolves symlinks; the prefix check keeps everything
        // inside the served root. It fails for more reasons than absence — a
        // directory on the way with no search bit on it is `PermissionDenied`,
        // and a mount whose server has gone is neither — so the reason is kept
        // rather than flattened to "nothing there".
        let canon = match self.host(path).canonicalize() {
            Ok(canon) => canon,
            Err(e) => return Err(ResolveError::of(&e)),
        };
        let Ok(rel) = canon.strip_prefix(&self.root) else {
            return Err(ResolveError::Outside);
        };
        // A canonical component that is not UTF-8 cannot ride in a VfsPath —
        // a lossy spelling would name a *different* (usually nonexistent)
        // path when joined back. Saying Missing is the honest answer: URLs
        // are UTF-8, so nothing could have addressed it faithfully anyway.
        let mut segs = Vec::new();
        for c in rel.components() {
            if let Component::Normal(os) = c {
                match os.to_str() {
                    Some(s) => segs.push(s.to_string()),
                    None => return Err(ResolveError::Missing),
                }
            }
        }
        Ok(VfsPath(segs))
    }

    fn metadata(&self, path: &VfsPath) -> io::Result<Meta> {
        let m = fs::metadata(self.host(path))?;
        #[cfg(unix)]
        let mode = Some(std::os::unix::fs::MetadataExt::mode(&m) & 0o7777);
        #[cfg(not(unix))]
        let mode = None;
        Ok(Meta {
            is_dir: m.is_dir(),
            is_file: m.is_file(),
            len: m.len(),
            mtime: m.modified().ok(),
            mode,
        })
    }

    fn read_dir(&self, path: &VfsPath) -> io::Result<Vec<Entry>> {
        let mut out = Vec::new();
        for de in fs::read_dir(self.host(path))?.flatten() {
            let meta = de.metadata().ok();
            out.push(Entry {
                name: de.file_name().to_string_lossy().into_owned(),
                is_dir: de.path().is_dir(), // follows symlinks
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                mtime: meta.and_then(|m| m.modified().ok()),
            });
        }
        Ok(out)
    }

    /// Bounded, the way `DocumentFs` and `SftpFs` are.
    ///
    /// Callers already ask only for files they have measured, so nothing today
    /// reaches this with a large one — but "the caller checked" is the kind of
    /// invariant that holds until a new caller does not, and this one is a whole
    /// file into memory on a machine that may be a phone. One byte past the cap
    /// is enough to tell the difference between a file that fits and one that
    /// does not, which is the answer `read`'s callers want anyway.
    fn read(&self, path: &VfsPath) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let mut out = Vec::new();
        fs::File::open(self.host(path))?
            .take(crate::util::MAX_HIGHLIGHT_BYTES + 1)
            .read_to_end(&mut out)?;
        Ok(out)
    }

    fn open(&self, path: &VfsPath) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(fs::File::open(self.host(path))?))
    }

    fn root_id_at(&self, path: &VfsPath) -> String {
        display_path(&self.host(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A desktop's own folder offers Delete; what `remove` does itself is the
    /// permanent fallback the shell asks about when the Trash will not take a
    /// thing — a file, or a folder with nothing in it, never the root.
    #[test]
    fn a_local_folder_deletes_a_file_or_an_empty_folder() {
        let dir = std::env::temp_dir().join(format!("ts-vfs-remove-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("full")).unwrap();
        fs::create_dir(dir.join("empty")).unwrap();
        fs::write(dir.join("a.txt"), b"x").unwrap();
        fs::write(dir.join("full").join("b"), b"y").unwrap();
        let v = LocalFs::new(dir.canonicalize().unwrap());
        let p = |n: &str| VfsPath::root().join(n);
        assert_eq!(v.deletable(), !cfg!(target_os = "android"));
        assert!(!v.writable(), "Delete alone; no Upload or New folder");
        assert!(v.local_path(&p("a.txt")).unwrap().ends_with("a.txt"));
        v.remove(&p("a.txt")).unwrap();
        v.remove(&p("empty")).unwrap();
        assert!(v.remove(&p("full")).is_err());
        assert_eq!(v.remove(&VfsPath::root()).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(!dir.join("a.txt").exists() && !dir.join("empty").exists() && dir.join("full").exists());
        // A link is the link: seen as one, and deleted as one, its target left.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("full"), dir.join("to-full")).unwrap();
            assert!(v.is_link(&p("to-full")) && !v.is_link(&p("full")));
            v.remove(&p("to-full")).unwrap();
            assert!(!dir.join("to-full").exists() && dir.join("full").join("b").exists());
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    /// One rule for a new name on every backend: trimmed, not empty, no
    /// separator of either kind, not `.` or `..`, no NUL, at most 255 bytes.
    #[test]
    fn a_new_name_is_one_name() {
        assert_eq!(new_name("  notes  "), Ok("notes"));
        assert_eq!(new_name("a b.txt"), Ok("a b.txt"));
        for bad in ["", "   ", ".", "..", "a/b", "a\\b", "a\0b"] {
            assert!(new_name(bad).is_err(), "{bad:?}");
        }
        assert!(new_name(&"x".repeat(255)).is_ok());
        assert!(new_name(&"x".repeat(256)).is_err());
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "treeserve-vfs-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_canonicalizes_and_confines() {
        let dir = tmp_dir("resolve").canonicalize().unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/a.txt"), "x").unwrap();
        let vfs = LocalFs::new(dir.clone());

        let p = VfsPath::new(vec!["sub".into(), "a.txt".into()]);
        let canon = vfs.resolve(&p).expect("resolves");
        assert_eq!(canon.segments(), ["sub", "a.txt"]);
        assert!(matches!(
            vfs.resolve(&VfsPath::new(vec!["nope".into()])),
            Err(ResolveError::Missing)
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    /// The symlink-escape rule `resolve_in_root` has always enforced: a link
    /// whose target is outside the served root resolves, and is refused.
    #[cfg(unix)]
    #[test]
    fn symlink_out_of_root_is_outside() {
        let outside = tmp_dir("out");
        fs::write(outside.join("secret"), "s").unwrap();
        let dir = tmp_dir("root").canonicalize().unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), dir.join("esc")).unwrap();
        let vfs = LocalFs::new(dir.clone());

        assert!(matches!(
            vfs.resolve(&VfsPath::new(vec!["esc".into()])),
            Err(ResolveError::Outside)
        ));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
    }
}
