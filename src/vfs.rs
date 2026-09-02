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

/// Why [`Vfs::resolve`] refused a path.
pub enum ResolveError {
    /// Nothing there.
    Missing,
    /// There, but it leads out of the served root — a symlink whose target is
    /// outside. Refused for the same reason `..` is refused in a URL.
    Outside,
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

    fn downloadable(&self) -> bool {
        Self::copies_are_worth_making()
    }

    fn resolve(&self, path: &VfsPath) -> Result<VfsPath, ResolveError> {
        // canonicalize resolves symlinks; the prefix check keeps everything
        // inside the served root.
        let Ok(canon) = self.host(path).canonicalize() else {
            return Err(ResolveError::Missing);
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

    fn read(&self, path: &VfsPath) -> io::Result<Vec<u8>> {
        fs::read(self.host(path))
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
        let canon = vfs.resolve(&p).ok().expect("resolves");
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
