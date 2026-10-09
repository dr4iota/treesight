use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn hexval(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_bytes(s: &str, plus_as_space: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hexval(b[i + 1]), hexval(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        if plus_as_space && b[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(b[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Decode a URL path component ('+' stays literal).
pub fn percent_decode(s: &str) -> String {
    percent_decode_bytes(s, false)
}

/// Percent-encode a single path segment or query value.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Encode a slash-separated relative path for use in an href.
pub fn href_path(rel: &[String]) -> String {
    let mut out = String::from("/");
    for (i, seg) in rel.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(&percent_encode(seg));
    }
    out
}

/// A filesystem path as a human would write it.
///
/// `canonicalize` on Windows returns verbatim paths — `\\?\C:\dir`, or
/// `\\?\UNC\server\share` for a network location — and that prefix has no place
/// in a page or a title. For display only: the stored root keeps the verbatim
/// form, because `resolve_in_root` compares it against freshly canonicalized
/// paths and the two spellings would never match.
pub fn display_path(p: &Path) -> String {
    let s = p.display().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(unc) => format!(r"\\{unc}"),
            None => rest.to_string(),
        },
        None => s,
    }
}

/// Parse "a=1&b=2" into pairs; keys and values are decoded ('+' becomes space).
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for kv in q.split('&') {
        if kv.is_empty() {
            continue;
        }
        let (k, v) = match kv.split_once('=') {
            Some((k, v)) => (k, v),
            None => (kv, ""),
        };
        out.push((
            percent_decode_bytes(k, true),
            percent_decode_bytes(v, true),
        ));
    }
    out
}

pub fn query_get<'a>(q: &'a [(String, String)], key: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Parse a Cookie header value ("a=1; b=2") into pairs.
pub fn parse_cookies(header: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in header.split(';') {
        if let Some((k, v)) = part.trim().split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

/// Shell-style glob match: `*`, `?`, `[abc]`, `[a-z]`, `[!...]`.
/// Iterative with single-star backtracking; matches over chars.
pub fn fnmatch(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None; // (pattern idx after '*', text mark)

    while ti < t.len() {
        let step = if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi + 1, ti));
                    pi += 1;
                    continue;
                }
                '?' => Some(pi + 1),
                '[' => match match_class(&p, pi, t[ti]) {
                    Some((true, next)) => Some(next),
                    Some((false, _)) => None,
                    // unterminated class: treat '[' as a literal
                    None => (p[pi] == t[ti]).then_some(pi + 1),
                },
                c => (c == t[ti]).then_some(pi + 1),
            }
        } else {
            None
        };
        match step {
            Some(next) => {
                pi = next;
                ti += 1;
            }
            None => match star {
                Some((sp, mark)) => {
                    star = Some((sp, mark + 1));
                    pi = sp;
                    ti = mark + 1;
                }
                None => return false,
            },
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Parse a `[...]` class starting at p[start]=='['. Returns (matched, index after ']').
/// None if the class is unterminated.
fn match_class(p: &[char], start: usize, c: char) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = matches!(p.get(i), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let cur = *p.get(i)?;
        if cur == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|&e| e != ']') {
            let end = p[i + 2];
            if cur <= c && c <= end {
                matched = true;
            }
            i += 3;
        } else {
            if cur == c {
                matched = true;
            }
            i += 1;
        }
    }
}

pub fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" | "md" | "markdown" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tif" | "tiff" => "image/tiff",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "ogv" => "video/ogg",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

pub fn ext_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

pub const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "avif", "svg", "bmp", "ico", "tif", "tiff",
];
pub const AUDIO_EXTS: &[&str] = &["mp3", "ogg", "oga", "opus", "wav", "flac", "m4a"];
pub const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "webm", "mkv", "mov", "ogv"];
pub const MARKDOWN_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd"];
pub const MERMAID_EXTS: &[&str] = &["mmd", "mermaid"];
/// Files larger than this are not syntax-highlighted, Markdown-rendered, or
/// shown as a directory README — the page offers Raw / Download instead.
pub const MAX_HIGHLIGHT_BYTES: u64 = 2 * 1024 * 1024;

pub fn human_size(n: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} B", n)
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

/// Format a mtime as "YYYY-MM-DD HH:MM", in the device's own time zone — the
/// one its clock and the system's file pickers show. This read UTC once, and
/// on a phone five hours off the reader's evening a file looked from tomorrow.
pub fn fmt_time(t: SystemTime) -> String {
    fmt_time_at(t, local_offset(t))
}

/// Seconds east of UTC at `t`, where the device is. UTC when the zone cannot
/// be read, which is what this always said before.
fn local_offset(t: SystemTime) -> i64 {
    use chrono::Offset;
    let at: chrono::DateTime<chrono::Utc> = t.into();
    i64::from(at.with_timezone(&chrono::Local).offset().fix().local_minus_utc())
}

/// [`fmt_time`] at a given offset from UTC, in seconds east.
fn fmt_time_at(t: SystemTime, offset: i64) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + offset;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// [`fmt_time`] for a narrow page: `MM-DD HH:MM` in `now`'s year, and the
/// date alone, `YYYY-MM-DD`, in any other.
pub fn fmt_time_short(t: SystemTime, now: SystemTime) -> String {
    fmt_time_short_at(t, now, local_offset(t))
}

fn fmt_time_short_at(t: SystemTime, now: SystemTime, offset: i64) -> String {
    let long = fmt_time_at(t, offset);
    match fmt_time_at(now, offset).get(..4) == long.get(..4) {
        true => long[5..].to_string(),
        false => long[..10].to_string(),
    }
}

/// How long ago `t` was, in words: *just now*, *2 minutes ago*, *1 day ago*.
/// A time after `now` is *just now* — a clock a little ahead is not news.
pub fn fmt_age(t: SystemTime, now: SystemTime) -> String {
    match age_parts(t, now) {
        (0, _) => "just now".to_string(),
        (1, unit) => format!("1 {unit} ago"),
        (n, unit) => format!("{n} {unit}s ago"),
    }
}

/// [`fmt_age`] for a narrow page: *now*, *2m ago*, *5h ago*, *1d ago*, *3w ago*.
pub fn fmt_age_short(t: SystemTime, now: SystemTime) -> String {
    match age_parts(t, now) {
        (0, _) => "now".to_string(),
        (n, "month") => format!("{n}mo ago"),
        (n, unit) => format!("{n}{} ago", &unit[..1]),
    }
}

/// The age as a count of its largest whole unit. Zero is under a minute.
fn age_parts(t: SystemTime, now: SystemTime) -> (u64, &'static str) {
    let secs = now.duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    const UNITS: [(u64, &str); 6] = [
        (365 * 86400, "year"),
        (30 * 86400, "month"),
        (7 * 86400, "week"),
        (86400, "day"),
        (3600, "hour"),
        (60, "minute"),
    ];
    for (size, unit) in UNITS {
        if secs >= size {
            return (secs / size, unit);
        }
    }
    (0, "minute")
}

// Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Heuristic: a buffer is binary if it contains a NUL byte.
pub fn looks_binary(buf: &[u8]) -> bool {
    buf.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_basics() {
        assert!(fnmatch("*.rs", "main.rs"));
        assert!(!fnmatch("*.rs", "main.rc"));
        assert!(fnmatch("a?c", "abc"));
        assert!(fnmatch("[a-c]x", "bx"));
        assert!(!fnmatch("[!a-c]x", "bx"));
        assert!(fnmatch("*", "anything"));
        assert!(fnmatch("foo*bar*baz", "foo_bar__baz"));
        assert!(!fnmatch("foo*bar", "foo_baz"));
        assert!(fnmatch("[", "[")); // unterminated class is literal
    }

    #[test]
    fn percent_roundtrip() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn short_times_and_ages() {
        use std::time::Duration;
        let at = |secs: u64| UNIX_EPOCH + Duration::from_secs(secs);
        // 2026-10-07 09:40 UTC, and a moment in the same year and another.
        let t = at(1_791_366_000);
        assert_eq!(fmt_time_at(t, 0), "2026-10-07 09:40");
        assert_eq!(fmt_time_short_at(t, at(1_791_366_000 + 86400), 0), "10-07 09:40");
        assert_eq!(fmt_time_short_at(t, at(1_791_366_000 + 120 * 86400), 0), "2026-10-07");
        // Four hours west, as on the tablet that found it; and a day boundary.
        assert_eq!(fmt_time_at(t, -4 * 3600), "2026-10-07 05:40");
        assert_eq!(fmt_time_at(t, -10 * 3600), "2026-10-06 23:40");
        let ago = |secs: u64| fmt_age(t, at(1_791_366_000 + secs));
        let short = |secs: u64| fmt_age_short(t, at(1_791_366_000 + secs));
        assert_eq!(ago(30), "just now");
        assert_eq!(short(30), "now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(150), "2 minutes ago");
        assert_eq!(short(150), "2m ago");
        assert_eq!(ago(5 * 3600), "5 hours ago");
        assert_eq!(short(5 * 3600), "5h ago");
        assert_eq!(ago(86400), "1 day ago");
        assert_eq!(short(86400), "1d ago");
        assert_eq!(short(21 * 86400), "3w ago");
        assert_eq!(short(70 * 86400), "2mo ago");
        assert_eq!(ago(400 * 86400), "1 year ago");
        // A clock a little ahead.
        assert_eq!(fmt_age(at(10), at(5)), "just now");
    }

    #[test]
    fn civil() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19723), (2024, 1, 1));
    }
}
