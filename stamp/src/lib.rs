//! What a built binary says it is: its version, the commit it was cut from,
//! and when it was compiled.
//!
//! Two halves that have to agree, so they live together. [`Stamp::emit`] is
//! called from a build script and hands the values to rustc as `rustc-env`;
//! [`BuildInfo`] and [`build_info!`] read the same names back at compile time
//! and give the program something to print. Neither half knows which app it is
//! working for — the prefix names that — so a downstream shell stamps itself
//! the same way this one does.
//!
//! **Nothing here may fail a build over a label.** No git on the machine, a
//! source tarball with no repository, a vendored checkout that was never
//! initialised: each leaves a value empty, and an empty value prints as
//! `unknown`. The one exception is deliberate — two version numbers that
//! disagree stop the build, because there is no right answer to pick from.

#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The identity of one built artifact, as [`Stamp`] wrote it.
///
/// Every field is `&'static str` because every field comes from `env!`, and
/// **an empty field is normal**: it means nothing could answer, not that
/// something went wrong. Build one with [`build_info!`] rather than by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    /// The product's name — `productName` from the Tauri config, or the crate.
    pub name: &'static str,
    /// The version that ships. On an app with a Tauri config that is the
    /// config's, which is the number the platform stores get.
    pub version: &'static str,
    /// Eight hex characters, or empty where git could not say.
    pub commit: &'static str,
    /// When that commit joined this history, RFC 3339.
    pub commit_time: &'static str,
    /// The branch it was built from; `HEAD` when detached.
    pub branch: &'static str,
    /// Tracked files differing from HEAD at build time, as a decimal count.
    /// Empty where it was not asked, `0` for a clean tree.
    pub dirty: &'static str,
    /// A checkout this build carries a copy of, for a shell that vendors
    /// another repository and wants its artifact to say which copy is inside.
    /// Empty in a build that vendors nothing.
    pub vendored: &'static str,
    /// That checkout's own commit, eight hex characters.
    pub vendored_commit: &'static str,
    /// When this binary was compiled, RFC 3339 in UTC.
    pub build_time: &'static str,
}

/// Reads back what [`Stamp`] emitted under `prefix` — the same string that was
/// passed to [`Stamp::prefix`], trailing underscore and all.
///
/// ```ignore
/// pub const BUILD: BuildInfo = treestamp::build_info!("TREESIGHT_");
/// ```
///
/// `env!` and not `option_env!`: the build script emits every one of these on
/// every build, empty where it had no answer, so a missing variable is a build
/// script that did not run rather than a value that is merely unknown — and
/// that is worth a compile error.
#[macro_export]
macro_rules! build_info {
    ($prefix:literal) => {
        $crate::BuildInfo {
            name: env!(concat!($prefix, "NAME")),
            version: env!(concat!($prefix, "VERSION")),
            commit: env!(concat!($prefix, "COMMIT")),
            commit_time: env!(concat!($prefix, "COMMIT_TIME")),
            branch: env!(concat!($prefix, "BRANCH")),
            dirty: env!(concat!($prefix, "DIRTY")),
            vendored: env!(concat!($prefix, "VENDORED")),
            vendored_commit: env!(concat!($prefix, "VENDORED_COMMIT")),
            build_time: env!(concat!($prefix, "BUILD_TIME")),
        }
    };
}

/// What an empty field prints as. One word, so a footer stays one line.
const UNKNOWN: &str = "unknown";

fn or_unknown(s: &str) -> &str {
    if s.is_empty() { UNKNOWN } else { s }
}

/// The date out of an RFC 3339 stamp, for somewhere with no room for the time.
/// Anything not shaped like one is passed through whole.
fn day(stamp: &str) -> &str {
    let head = stamp.get(..10).unwrap_or(stamp);
    let b = head.as_bytes();
    if b.len() == 10 && b[4] == b'-' && b[7] == b'-' {
        head
    } else {
        stamp
    }
}

impl BuildInfo {
    /// For a build nobody stamped — what a test gets for free, and what a
    /// consumer falls back to before it has been handed anything.
    pub const UNKNOWN: BuildInfo = BuildInfo {
        name: "",
        version: "",
        commit: "",
        commit_time: "",
        branch: "",
        dirty: "",
        vendored: "",
        vendored_commit: "",
        build_time: "",
    };

    /// `a1b2c3d4`, or `a1b2c3d4+2` where two tracked files differed from it.
    ///
    /// The `+n` matters more than it looks: a hash alone claims the artifact is
    /// that commit, and a build from a dirty tree is not.
    pub fn commit_mark(&self) -> String {
        if self.commit.is_empty() {
            return UNKNOWN.to_string();
        }
        match self.dirty.parse::<u32>() {
            Ok(n) if n > 0 => format!("{}+{n}", self.commit),
            _ => self.commit.to_string(),
        }
    }

    /// `treesight v0.1.0 (a1b2c3d4+2)` — the status-line form, and the same
    /// string `treeserve`'s page footer shows.
    ///
    /// Parentheses around the commit because the footer already spends `·` on
    /// the gap between this label and the path beside it, and a separator that
    /// means two things at once means neither. The parentheses are dropped
    /// rather than left empty where there is no commit to put in them.
    pub fn label(&self) -> String {
        let head = format!("{} v{}", or_unknown(self.name), or_unknown(self.version));
        match self.commit.is_empty() {
            true => head,
            false => format!("{head} ({})", self.commit_mark()),
        }
    }

    /// One line for a log: the label, what it vendors, and when it was made.
    /// On Android this is a logcat line, which is how a sideloaded build can
    /// be identified without opening it.
    pub fn line(&self) -> String {
        let mut s = self.label();
        if !self.vendored_commit.is_empty() {
            s.push_str(&format!(
                " · {} {}",
                or_unknown(self.vendored),
                self.vendored_commit
            ));
        }
        if !self.build_time.is_empty() {
            s.push_str(&format!(" · built {}", day(self.build_time)));
        }
        s
    }

    /// Several lines, for `--version`, where there is room to say all of it.
    pub fn report(&self) -> String {
        let dirty = match self.dirty.parse::<u32>() {
            Ok(0) => " (clean)".to_string(),
            Ok(1) => " (1 file changed)".to_string(),
            Ok(n) => format!(" ({n} files changed)"),
            Err(_) => String::new(),
        };
        let branch = match self.branch.is_empty() {
            true => String::new(),
            false => format!(" on {}", self.branch),
        };
        let vendored = match self.vendored.is_empty() {
            true => String::new(),
            false => format!(
                "\n{:<10} {}",
                self.vendored,
                or_unknown(self.vendored_commit)
            ),
        };
        format!(
            "{} {}\n\
             commit     {}{branch}{dirty}\n\
             committed  {}{vendored}\n\
             built      {}",
            or_unknown(self.name),
            or_unknown(self.version),
            or_unknown(self.commit),
            or_unknown(self.commit_time),
            or_unknown(self.build_time),
        )
    }
}

/// What a build script asks for, and where to look for it.
///
/// ```ignore
/// treestamp::Stamp::new(env!("CARGO_MANIFEST_DIR"), "TREESIGHT_").emit();
/// ```
pub struct Stamp {
    app: PathBuf,
    repo: PathBuf,
    prefix: String,
    vendored: Option<(String, PathBuf)>,
}

impl Stamp {
    /// `app` is the crate directory a `tauri.conf.json` sits in — normally
    /// `CARGO_MANIFEST_DIR`. Git is asked about its parent, which is the
    /// repository the app lives in rather than the crate.
    ///
    /// `prefix` starts every variable name and is what [`build_info!`] is
    /// given: `"TREESIGHT_"`, trailing underscore included.
    pub fn new(app: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        let app = app.into();
        let repo = app.parent().unwrap_or(&app).to_path_buf();
        Stamp { app, repo, prefix: prefix.into(), vendored: None }
    }

    /// Look for the repository somewhere other than the crate's parent.
    pub fn repo(mut self, dir: impl Into<PathBuf>) -> Self {
        self.repo = dir.into();
        self
    }

    /// Also stamp a checkout this build carries a copy of, under a name to
    /// print it by. A shell that vendors another repository as a submodule
    /// names it here, and its artifacts then say which copy is inside.
    pub fn vendored(mut self, name: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        self.vendored = Some((name.into(), dir.into()));
        self
    }

    /// Emit the lot. Call this from `main` in a build script.
    ///
    /// Every value may be overridden by an environment variable of the same
    /// name, which is how a build script that runs cargo several times — four
    /// Android ABIs in one invocation — makes them all agree about when they
    /// were built. An empty override counts as absent, so a driver may export
    /// the whole set and leave the ones it has nothing to say about blank.
    pub fn emit(&self) {
        let p = &self.prefix;
        let (name, version) = self.identity();
        println!("cargo:rustc-env={p}NAME={name}");
        println!("cargo:rustc-env={p}VERSION={version}");

        // `%cI` and not `%aI`: the author date survives a rebase, so on a
        // rebased branch it can predate the code that is in the artifact. What
        // is wanted is when this commit joined this history.
        self.emit_one("COMMIT", || git(&self.repo, &["rev-parse", "--short=8", "HEAD"]));
        self.emit_one("COMMIT_TIME", || git(&self.repo, &["log", "-1", "--format=%cI"]));
        self.emit_one("BRANCH", || git(&self.repo, &["rev-parse", "--abbrev-ref", "HEAD"]));
        // Tracked files differing from HEAD, as a count. Untracked files are
        // not in it: a build directory nobody added is not a modified build.
        self.emit_one("DIRTY", || {
            git(&self.repo, &["diff", "--name-only", "HEAD"])
                .map(|out| out.lines().filter(|l| !l.is_empty()).count().to_string())
        });
        let vendored = self.vendored.as_ref();
        self.emit_one("VENDORED", || vendored.map(|(name, _)| name.clone()));
        self.emit_one("VENDORED_COMMIT", || {
            git(&vendored?.1, &["rev-parse", "--short=8", "HEAD"])
        });
        // SOURCE_DATE_EPOCH is the reproducible-builds way of saying "pretend
        // it is this moment", and pins this value for a build that has to come
        // out the same twice.
        self.emit_one("BUILD_TIME", || {
            let secs = std::env::var("SOURCE_DATE_EPOCH")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(now);
            Some(rfc3339(secs))
        });
        println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

        // A build script does not rerun because you committed. Emitting any
        // `rerun-if-changed` at all — and `tauri_build::build` emits plenty —
        // replaces cargo's default "rerun when a file in the package changed",
        // so without these a new commit would leave last week's hash in the
        // binary. The environment overrides cover a build script that computes
        // the values itself; these cover a bare `cargo build`.
        watch_head(&self.repo);
        if let Some((_, dir)) = vendored {
            watch_head(dir);
        }
    }

    /// The product's name and version.
    ///
    /// From `tauri.conf.json` where there is one, because that file is what the
    /// platform stores read: an Android `versionCode` is derived from its
    /// version, and dropping the field does not fall back to Cargo — it writes
    /// no version at all and Gradle ships its own default. Cargo insists on a
    /// version too, so **the two are checked against each other here**: two
    /// numbers that can differ is one number nobody can trust, and the build is
    /// where the second one gets said out loud.
    fn identity(&self) -> (String, String) {
        let cargo_name = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
        let cargo_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let path = self.app.join("tauri.conf.json");
        if !path.is_file() {
            return (cargo_name, cargo_version);
        }
        println!("cargo:rerun-if-changed={}", path.display());
        // Hand-read rather than parsed: a `serde_json` here is a dependency
        // every build of every consumer pays for before its own build starts,
        // for two strings out of a file the Tauri build script parses properly
        // moments later. A field this cannot find is a field left to Cargo,
        // and a malformed file is `tauri_build`'s error to report in the words
        // it has for it.
        let text = fs::read_to_string(&path).unwrap_or_default();
        let name = json_string(&text, "productName").unwrap_or(cargo_name);
        let Some(version) = json_string(&text, "version") else {
            return (name, cargo_version);
        };
        assert!(
            version == cargo_version,
            "{} says version {version} and Cargo.toml says {cargo_version}. \
             The config's is the one that ships — an Android versionCode is \
             derived from it, and it is what the page footer shows — so a \
             different number in Cargo.toml is a second answer with nothing to \
             be right about. Bump both.",
            path.display()
        );
        (name, version)
    }

    /// Reads the environment first, falls back to `f`, and hands rustc the
    /// result. An empty override counts as absent.
    fn emit_one(&self, key: &str, f: impl FnOnce() -> Option<String>) {
        let name = format!("{}{key}", self.prefix);
        println!("cargo:rerun-if-env-changed={name}");
        let value = match std::env::var(&name) {
            Ok(v) if !v.is_empty() => v,
            _ => f().unwrap_or_default(),
        };
        // A newline would end the directive and turn the rest into a line cargo
        // does not understand. Nothing here should contain one, and a value
        // that does is one to flatten rather than mangle the build with.
        let value = value.replace(['\n', '\r'], " ");
        println!("cargo:rustc-env={name}={value}");
    }
}

/// The string value of a top-level `"key": "value"` pair. Enough for two
/// fields of a file whose shape is fixed, and honest about the rest: anything
/// it does not recognise it declines to answer for.
fn json_string(text: &str, key: &str) -> Option<String> {
    let at = text.find(&format!("\"{key}\""))? + key.len() + 2;
    let rest = text[at..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    // An escape would mean the value is not what the bytes before the quote
    // say. Neither of the two fields read here may contain one, so a value
    // that does is a file this has no business guessing about.
    let value = &rest[..end];
    (!value.contains('\\')).then(|| value.to_string())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `git` in `dir`, or `None` — which covers no git on the PATH, no repository,
/// a repository with no commits, and a checkout that was never initialised.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    if !dir.is_dir() {
        return None;
    }
    let out = Command::new("git").args(args).current_dir(dir).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Rebuild when this repository's HEAD moves: the file itself, the branch ref
/// it names, and `packed-refs` for a ref that has been packed away.
///
/// Only paths that exist are named. Cargo treats a `rerun-if-changed` on a
/// missing path as "rerun always", which would recompile on every build of a
/// tree with no git in it — the one case this must stay quiet for.
fn watch_head(dir: &Path) {
    let Some(gitdir) = git(dir, &["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let gitdir = PathBuf::from(gitdir);
    let head = gitdir.join("HEAD");
    // A worktree and a submodule both put HEAD somewhere other than beside the
    // refs, so a ref is resolved against the *common* dir where there is one.
    let common = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .map(PathBuf::from)
        .unwrap_or_else(|| gitdir.clone());
    if let Ok(text) = fs::read_to_string(&head)
        && let Some(reference) = text.trim().strip_prefix("ref: ")
    {
        watch(&common.join(reference));
    }
    watch(&head);
    watch(&common.join("packed-refs"));
}

fn watch(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// Seconds since the epoch as `2026-08-28T09:12:31Z`.
///
/// Hand-rolled rather than a date crate: this is the only date either half
/// formats, and it formats it once per build. The civil-from-days arithmetic is
/// Howard Hinnant's, exact for every year this will ever see.
fn rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: BuildInfo = BuildInfo {
        name: "treesight",
        version: "0.1.0",
        commit: "a1b2c3d4",
        commit_time: "2026-08-28T09:12:31+08:00",
        branch: "main",
        dirty: "0",
        vendored: "",
        vendored_commit: "",
        build_time: "2026-08-28T01:12:44Z",
    };

    #[test]
    fn a_clean_tree_is_the_hash_alone() {
        assert_eq!(FULL.commit_mark(), "a1b2c3d4");
        assert_eq!(FULL.label(), "treesight v0.1.0 (a1b2c3d4)");
    }

    #[test]
    fn a_dirty_tree_says_how_dirty() {
        let b = BuildInfo { dirty: "2", ..FULL };
        assert_eq!(b.label(), "treesight v0.1.0 (a1b2c3d4+2)");
    }

    /// The whole point of the fallbacks: no git, no repository, nothing
    /// vendored — still a label, still a report, and no empty parentheses.
    #[test]
    fn nothing_known_still_renders() {
        let b = BuildInfo { name: "treesight", version: "0.1.0", ..BuildInfo::UNKNOWN };
        assert_eq!(b.label(), "treesight v0.1.0");
        let report = b.report();
        assert!(report.contains("commit     unknown"), "{report}");
        assert!(!report.contains(" on "), "no branch, so no branch clause: {report}");
    }

    #[test]
    fn a_vendored_checkout_is_named_and_dated() {
        let b = BuildInfo { vendored: "treesight", vendored_commit: "5f6a7b8c", ..FULL };
        assert_eq!(
            b.line(),
            "treesight v0.1.0 (a1b2c3d4) · treesight 5f6a7b8c · built 2026-08-28"
        );
        assert!(b.report().contains("treesight  5f6a7b8c"), "{}", b.report());
    }

    #[test]
    fn a_build_time_that_is_not_a_date_is_not_truncated() {
        let b = BuildInfo { build_time: "whenever", ..FULL };
        assert!(b.line().ends_with("built whenever"), "{}", b.line());
    }

    #[test]
    fn the_two_fields_are_read_out_of_a_tauri_config() {
        let text = r#"{ "$schema": "x", "productName": "treesight", "version": "1.2.3" }"#;
        assert_eq!(json_string(text, "productName").as_deref(), Some("treesight"));
        assert_eq!(json_string(text, "version").as_deref(), Some("1.2.3"));
        assert_eq!(json_string(text, "identifier"), None);
    }

    /// Whitespace is the config's business, not ours; an escape is a value this
    /// declines to answer for rather than one it gets subtly wrong.
    #[test]
    fn odd_spacing_reads_and_an_escape_does_not() {
        assert_eq!(json_string("{\"version\"\n  :\t\"9.9.9\"}", "version").as_deref(), Some("9.9.9"));
        assert_eq!(json_string(r#"{"productName": "a\"b"}"#, "productName"), None);
    }

    #[test]
    fn the_epoch_formats_as_the_epoch() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20Z");
        // A leap day, which is where the civil-from-days arithmetic earns its
        // keep: 2024 is a leap year, 1900 was not, 2000 was.
        assert_eq!(rfc3339(1_709_164_800), "2024-02-29T00:00:00Z");
    }
}
