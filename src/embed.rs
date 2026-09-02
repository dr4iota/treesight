//! A tree that is only in the binary.
//!
//! The pages a program ships to explain itself have no folder on the machine
//! running it, and want none: they are versioned with the program, they cannot
//! go missing, and reaching them asks nothing of the platform — no grant, no
//! network, no install step. So they are compiled in and served through the same
//! seam as any other root, which is what makes them look like ordinary files to
//! every view in the crate.

use std::collections::BTreeMap;
use std::io::{self, Cursor};

use crate::vfs::{Entry, Meta, ReadSeek, ResolveError, Vfs, VfsPath};

/// Files compiled into the program, served as a root.
///
/// Paths are `/`-joined and relative to the root. A later pair with the same
/// path replaces an earlier one, which is the whole of how a downstream shell
/// rewrites one page of an upstream set without copying the rest of it.
pub struct EmbeddedFs {
    scheme: &'static str,
    files: BTreeMap<String, &'static [u8]>,
}

impl EmbeddedFs {
    pub fn new(
        scheme: &'static str,
        files: impl IntoIterator<Item = (&'static str, &'static [u8])>,
    ) -> EmbeddedFs {
        EmbeddedFs {
            scheme,
            files: files
                .into_iter()
                .map(|(path, bytes)| (path.trim_start_matches('/').to_string(), bytes))
                .collect(),
        }
    }

    fn key(path: &VfsPath) -> String {
        path.segments().join("/")
    }

    /// A directory here is a prefix something is filed under. There are no
    /// directory entries to hold, so the shape of the tree is whatever the paths
    /// imply — and the root is a directory even when nothing has been filed yet.
    fn is_dir(&self, key: &str) -> bool {
        if key.is_empty() {
            return true;
        }
        let prefix = format!("{key}/");
        self.files.keys().any(|k| k.starts_with(&prefix))
    }
}

impl Vfs for EmbeddedFs {
    /// The pages are `include_bytes!`d into this binary: they did not travel to
    /// get here, and the only author is whoever built it.
    fn on_this_device(&self) -> bool {
        true
    }

    /// Never. These pages are compiled into the binary the reader is running —
    /// there is no copy to be had that they do not already have.
    fn downloadable(&self) -> bool {
        false
    }

    /// `Outside` cannot happen: there are no links here to lead anywhere, and a
    /// path that names nothing is simply missing.
    fn resolve(&self, path: &VfsPath) -> Result<VfsPath, ResolveError> {
        let key = Self::key(path);
        match self.files.contains_key(&key) || self.is_dir(&key) {
            true => Ok(path.clone()),
            false => Err(ResolveError::Missing),
        }
    }

    /// No `mtime`, which the seam allows and which is honest: these bytes are as
    /// old as the build, and a page served without a conditional request beats
    /// one served with a wrong answer to it.
    fn metadata(&self, path: &VfsPath) -> io::Result<Meta> {
        let key = Self::key(path);
        if let Some(bytes) = self.files.get(&key) {
            return Ok(Meta {
                is_dir: false,
                is_file: true,
                len: bytes.len() as u64,
                mtime: None,
                mode: None,
            });
        }
        match self.is_dir(&key) {
            true => Ok(Meta {
                is_dir: true,
                is_file: false,
                len: 0,
                mtime: None,
                mode: None,
            }),
            false => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    fn read_dir(&self, path: &VfsPath) -> io::Result<Vec<Entry>> {
        let key = Self::key(path);
        if !self.is_dir(&key) {
            return Err(io::Error::other("not a directory"));
        }
        let prefix = match key.is_empty() {
            true => String::new(),
            false => format!("{key}/"),
        };
        // Keyed, so a directory named by four files below it is one entry. Sorted
        // by the map rather than here: ordering rows is the page's job, and this
        // only has to stop naming the same child twice.
        let mut children: BTreeMap<&str, (bool, u64)> = BTreeMap::new();
        for (path, bytes) in &self.files {
            let Some(rest) = path.strip_prefix(prefix.as_str()) else {
                continue;
            };
            match rest.split_once('/') {
                Some((dir, _)) => {
                    children.entry(dir).or_insert((true, 0));
                }
                None => {
                    children.insert(rest, (false, bytes.len() as u64));
                }
            }
        }
        Ok(children
            .into_iter()
            .map(|(name, (is_dir, size))| Entry {
                name: name.to_string(),
                is_dir,
                size,
                mtime: None,
            })
            .collect())
    }

    fn read(&self, path: &VfsPath) -> io::Result<Vec<u8>> {
        self.files
            .get(&Self::key(path))
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn open(&self, path: &VfsPath) -> io::Result<Box<dyn ReadSeek>> {
        let bytes = self
            .files
            .get(&Self::key(path))
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(Box::new(Cursor::new(*bytes)))
    }

    /// The whole tree, whatever is asked about: an embedded set is served as one
    /// root and a directory inside it is not a root of its own. A nested set that
    /// wanted to be would be a second `EmbeddedFs` over the same table, which is
    /// the honest way to say it rather than handing out an id nothing can open.
    fn root_id_at(&self, _path: &VfsPath) -> String {
        format!("{}:/", self.scheme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGES: &[(&str, &[u8])] = &[
        ("index.md", b"one"),
        ("keyboard.md", b"two"),
        ("deep/nested.md", b"three"),
    ];

    fn fs() -> EmbeddedFs {
        EmbeddedFs::new("usage", PAGES.iter().copied())
    }

    fn at(segments: &[&str]) -> VfsPath {
        VfsPath::new(segments.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn a_directory_is_whatever_the_paths_imply() {
        let fs = fs();
        let names: Vec<String> = fs
            .read_dir(&VfsPath::root())
            .expect("root lists")
            .into_iter()
            .map(|e| format!("{}{}", e.name, if e.is_dir { "/" } else { "" }))
            .collect();
        assert_eq!(names, ["deep/", "index.md", "keyboard.md"]);
        assert!(fs.metadata(&at(&["deep"])).expect("deep is there").is_dir);
        assert_eq!(fs.read(&at(&["deep", "nested.md"])).unwrap(), b"three");
    }

    /// The point of the whole type: a downstream set replaces one page by name
    /// and inherits the rest.
    #[test]
    fn a_later_page_replaces_an_earlier_one() {
        let extra: &[(&str, &[u8])] = &[("index.md", b"mine")];
        let fs = EmbeddedFs::new("usage", PAGES.iter().chain(extra).copied());
        assert_eq!(fs.read(&at(&["index.md"])).unwrap(), b"mine");
        assert_eq!(fs.read(&at(&["keyboard.md"])).unwrap(), b"two");
    }

    #[test]
    fn nothing_here_leads_outside() {
        let fs = fs();
        assert!(matches!(
            fs.resolve(&at(&["absent.md"])),
            Err(ResolveError::Missing)
        ));
        assert!(fs.resolve(&at(&["deep"])).is_ok());
        assert_eq!(fs.root_id_at(&at(&["deep"])), "usage:/");
    }
}
