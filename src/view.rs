use crate::data::{delimiter_for, gunzip, parse_delimited, reformat_json, render_table, MAX_TABLE_ROWS};
use crate::md::{render_markdown, render_mermaid_figure, RawHtml};
use crate::vfs::Vfs;
use crate::page::{
    bare_layout, flag, layout, svg_icon, Prefs, ICON_DOWNLOAD, ICON_PRINT, ICON_PREVIEW, ICON_RAW,
    ICON_SOURCE,
};
use crate::util::*;
use crate::vfs::VfsPath;
use std::io::Read;
use crate::{Root, State};

/// The bytes, for something on a page to point at. `bare` says so in the URL
/// rather than leaving it to be worked out from the request's headers: what a
/// browser says it accepts for a picture or a frame is its own business, and
/// the answer here has to be the file either way.
fn raw_href(rel: &[String]) -> String {
    format!("{}?raw=1&bare=1", href_path(rel))
}

/// Print, on the views that are documents: a rendered page and a listing of
/// lines both go to paper as what they are, where a picture, a film and a file
/// in a frame are either the engine's business or nothing anyone prints.
///
/// Shell only, for two reasons that agree. A browser prints on Ctrl+P and from
/// its own menu, and the shell's window has neither. And a link cannot print
/// anything by itself: the shell reads this one and prints, the same way it
/// reads Back and Refresh, where on the web it would take the first line of
/// JavaScript this app has ever needed.
fn print_flag(state: &State) -> String {
    if state.cfg.app_ui {
        flag(
            "",
            "/.ts/print",
            &svg_icon(ICON_PRINT),
            "Print",
            "Print (Ctrl+P)",
        )
    } else {
        String::new()
    }
}

fn std_controls(state: &State, rel: &[String], vfs: &dyn Vfs) -> String {
    let base = href_path(rel);
    format!(
        "{}{}{}",
        flag(
            "",
            &format!("{base}?raw=1"),
            &svg_icon(ICON_RAW),
            "Raw",
            "The file as it is on disk"
        ),
        download_flag(&base, vfs),
        file_actions(state, rel, vfs)
    )
}

/// The backend's verbs that belong on a file's own page as well as under its
/// row — Keep offline, say — after Download, lit when on. Links the shell
/// claims, so only in the app; the token is the page's.
fn file_actions(state: &State, rel: &[String], vfs: &dyn Vfs) -> String {
    if !state.cfg.app_ui {
        return String::new();
    }
    let Some(info) = vfs.row_info(&VfsPath::new(rel.to_vec())) else {
        return String::new();
    };
    let token = &state.cfg.action_token;
    info.actions
        .iter()
        .filter(|a| a.on_file_page)
        .map(|a| {
            flag(
                if a.on { "on" } else { "" },
                &format!(
                    "/.ts/act?path={}&a={}&t={}",
                    crate::util::percent_encode(&href_path(rel)),
                    crate::util::percent_encode(&a.id),
                    token
                ),
                &svg_icon(&a.icon),
                &a.label,
                &a.title,
            )
        })
        .collect()
}

/// The same, in a sentence rather than on the flag row: a finished link, or
/// nothing at all. A page that says "or download" and then does not is worse
/// than one that says nothing, so the caller composes around what it gets back
/// rather than pasting fragments either side of it.
fn download_link(rel: &[String], vfs: &dyn Vfs, words: &str) -> Option<String> {
    vfs.downloadable().then(|| {
        format!(
            "<a href=\"{}?dl=1\">{words}</a>",
            html_escape(&href_path(rel))
        )
    })
}

/// Download, where a copy is a thing worth having. See [`Vfs::downloadable`].
fn download_flag(base: &str, vfs: &dyn Vfs) -> String {
    match vfs.downloadable() {
        true => flag(
            "",
            &format!("{base}?dl=1"),
            &svg_icon(ICON_DOWNLOAD),
            "Download",
            "Save a copy",
        ),
        false => String::new(),
    }
}

fn meta_line(size: u64, mtime: Option<std::time::SystemTime>, extra: &str) -> String {
    format!(
        "<p class=\"filemeta\">{}{} &middot; {}</p>",
        human_size(size),
        mtime
            .map(|t| format!(" &middot; {}", fmt_time(t)))
            .unwrap_or_default(),
        extra
    )
}

/// The raw view: the file itself, in a frame that has the window from the header
/// down. The frame keeps whatever the engine does with those bytes — the text
/// viewer's wrapping, the PDF viewer, an image at its own size — and none of the
/// furniture the other views put around their content, since none of it is the
/// file. What the header says and what a flag does is ours; the rest is not.
pub fn raw_page(
    state: &State,
    root: &Root,
    prefs: Prefs<'_>,
    rel: &[String],
    canon: &VfsPath,
    url_now: &str,
) -> String {
    let name = rel.last().map(String::as_str).unwrap_or("");
    let base = href_path(rel);
    // The way out is the way in reversed: where every other view offers Raw,
    // this one offers the view it came from, named for what that is (`way_back`).
    // Download stays — it is the other thing wanted of a file one is looking at
    // as a file.
    let back = way_back(root.vfs.as_ref(), canon, name)
        .map(|(icon, label, hint)| flag("", &base, &svg_icon(icon), label, hint))
        .unwrap_or_default();
    let controls = format!(
        "{}{}",
        back,
        flag(
            "",
            &format!("{base}?dl=1"),
            &svg_icon(ICON_DOWNLOAD),
            "Download",
            "Save a copy"
        )
    );
    // `sandbox` with no allowances: the framed document may be a remote
    // machine's HTML served as itself, and the parent is a page that can
    // reach the shell. `serve_raw` sends CSP `sandbox` on such documents
    // too — attribute and header each cover an engine that misses the other,
    // and both cost nothing.
    let content = format!(
        "<iframe class=\"raw\" sandbox src=\"{}\" title=\"{}\"></iframe>",
        html_escape(&raw_href(rel)),
        html_escape(name)
    );
    bare_layout(state, root, prefs, rel, url_now, &controls, &content)
}

/// The name a `.gz` had before it was packed, where the page opens it to
/// show: `syslog.2.gz` is `syslog.2`. Not a picture, a film, a sound or a PDF
/// packed — those are drawn by an element pointed at the file's bytes, which
/// are still the packed ones, so `photo.png.gz` is an ordinary `.gz` and gets
/// the binary notice and Download.
fn unpacked_name(name: &str) -> Option<&str> {
    if ext_of(name) != "gz" {
        return None;
    }
    let inner = &name[..name.len() - 3];
    let ext = ext_of(inner);
    match [IMAGE_EXTS, AUDIO_EXTS, VIDEO_EXTS].iter().any(|exts| exts.contains(&ext.as_str())) || ext == "pdf" {
        true => None,
        false => Some(inner),
    }
}

/// The view `file_page` draws for this file, as Raw's way back names it: Preview
/// where the app draws, plays or renders the file, Source where it shows the
/// text — or nothing, where that page is a notice (too large, binary,
/// unreadable): a notice is not a view, and Raw is then the only one there is,
/// as the header sketch's table has it. Up and the system's Back still leave.
/// The same tests in the same order as `file_page`, which is the page this
/// names, except that the binary test reads only the 8 KiB it looks at: the
/// frame fetches the whole file anyway, and a remote tree should not send it
/// twice to name a button.
fn way_back(
    vfs: &dyn Vfs,
    canon: &VfsPath,
    name: &str,
) -> Option<(&'static str, &'static str, &'static str)> {
    const PREVIEW: (&str, &str, &str) = (ICON_PREVIEW, "Preview", "The file as this app shows it");
    let ext = ext_of(name);
    let size = vfs.metadata(canon).map(|m| m.len).unwrap_or(0);
    if vfs.open_limit().is_some_and(|limit| size > limit) {
        return None;
    }
    if [IMAGE_EXTS, AUDIO_EXTS, VIDEO_EXTS].iter().any(|exts| exts.contains(&ext.as_str())) || ext == "pdf" {
        return Some(PREVIEW);
    }
    if size > MAX_HIGHLIGHT_BYTES {
        return None;
    }
    let mut head = Vec::with_capacity(8192);
    vfs.open(canon).ok()?.take(8192).read_to_end(&mut head).ok()?;
    // A `.gz` is named for what is inside, and `file_page` refuses one past the
    // limit unpacked, which only unpacking that far finds out: the one case
    // here that reads more than the head, and only up to what the page would.
    // One that is not gzip after all is its own bytes, as it is there.
    let ext = match unpacked_name(name) {
        Some(inner) => match gunzip(vfs.open(canon).ok()?, MAX_HIGHLIGHT_BYTES) {
            Some((_, true)) => return None,
            Some((mut bytes, false)) => {
                bytes.truncate(8192);
                head = bytes;
                ext_of(inner)
            }
            None => ext,
        },
        None => ext,
    };
    if looks_binary(&head) {
        return None;
    }
    // JSON says Preview without reading the rest to find out whether it is
    // JSON: one that is not lands on the source, under a name that oversells it.
    if [MARKDOWN_EXTS, MERMAID_EXTS, TABLE_EXTS, JSON_EXTS].iter().any(|exts| exts.contains(&ext.as_str())) {
        return Some(PREVIEW);
    }
    Some((ICON_SOURCE, "Source", "Highlighted source"))
}

pub fn file_page(
    state: &State,
    root: &Root,
    prefs: Prefs<'_>,
    rel: &[String],
    canon: &VfsPath,
    query: &[(String, String)],
    url_now: &str,
) -> String {
    let vfs = root.vfs.as_ref();
    let name = rel.last().map(String::as_str).unwrap_or("");
    let ext = ext_of(name);
    let meta = vfs.metadata(canon).ok();
    let size = meta.as_ref().map(|m| m.len).unwrap_or(0);
    let mtime = meta.and_then(|m| m.mtime);

    // Past what this backend will hand over whole, nothing below can be drawn:
    // every branch there points an element or a reader at the bytes, and an
    // element pointed at a refusal is a broken box with nothing to read in it.
    // No controls either — Raw and Download go the same way this would have.
    if vfs.open_limit().is_some_and(|limit| size > limit) {
        let content = format!(
            "{}<div class=\"bigmsg\"><p>File is too large to show here ({}).</p>\
             <p>Open it in an app on this device that reads them.</p></div>",
            meta_line(size, mtime, "large file"),
            human_size(size)
        );
        return layout(state, root, prefs, rel, url_now, "", false, &content);
    }

    if IMAGE_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<div class=\"fit\"><a href=\"{1}\"><img class=\"preview-img\" src=\"{1}\" alt=\"{2}\"></a></div>",
            meta_line(size, mtime, "image"),
            html_escape(&raw_href(rel)),
            html_escape(name)
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(state, rel, vfs), false, &content);
    }
    if VIDEO_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<div class=\"fit\"><video class=\"preview\" controls src=\"{}\"></video></div>",
            meta_line(size, mtime, "video"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(state, rel, vfs), false, &content);
    }
    if AUDIO_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<audio class=\"preview\" controls src=\"{}\"></audio>",
            meta_line(size, mtime, "audio"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(state, rel, vfs), false, &content);
    }
    if ext == "pdf" {
        let content = format!(
            "{}<div class=\"fit\"><embed class=\"pdf\" src=\"{}\" type=\"application/pdf\"></div>",
            meta_line(size, mtime, "pdf"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(state, rel, vfs), false, &content);
    }

    // Text-ish content from here on.
    if size > MAX_HIGHLIGHT_BYTES {
        let raw = format!("<a href=\"{}?raw=1\">View raw</a>", html_escape(&href_path(rel)));
        let ways = match download_link(rel, vfs, "download") {
            Some(dl) => format!("{raw} or {dl}."),
            None => format!("{raw}."),
        };
        let content = format!(
            "{}<div class=\"bigmsg\"><p>File is too large to render ({}).</p><p>{ways}</p></div>",
            meta_line(size, mtime, "large file"),
            human_size(size)
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(state, rel, vfs), false, &content);
    }

    let Ok(bytes) = vfs.read(canon) else {
        let content = "<div class=\"bigmsg\"><p>Could not read file.</p></div>".to_string();
        return layout(state, root, prefs, rel, url_now, "", false, &content);
    };

    // A `.gz` is read as what it holds, under the name it had before it was
    // packed, so `syslog.2.gz` is a log and `rows.csv.gz` a table; Raw and
    // Download still hand over the packed file. One that is not gzip after all
    // falls through as its own bytes, which is the binary notice.
    let (bytes, name, ext, unpacked) = match unpacked_name(name) {
        Some(inner_name) => match gunzip(&bytes[..], MAX_HIGHLIGHT_BYTES) {
            Some((inner, false)) => {
                let len = inner.len() as u64;
                (inner, inner_name, ext_of(inner_name), Some(len))
            }
            Some((_, true)) => {
                let raw = format!("<a href=\"{}?raw=1\">View raw</a>", html_escape(&href_path(rel)));
                let ways = match download_link(rel, vfs, "download") {
                    Some(dl) => format!("{raw} or {dl}."),
                    None => format!("{raw}."),
                };
                let content = format!(
                    "{}<div class=\"bigmsg\"><p>File is too large to render once unpacked \
                     (over {}).</p><p>{ways}</p></div>",
                    meta_line(size, mtime, "gzip"),
                    human_size(MAX_HIGHLIGHT_BYTES)
                );
                return layout(state, root, prefs, rel, url_now, &std_controls(state, rel, vfs), false, &content);
            }
            None => (bytes, name, ext, None),
        },
        None => (bytes, name, ext, None),
    };
    // What the meta line adds for a packed file: how big it is opened.
    let unpacked = unpacked
        .map(|n| format!(" &middot; gzip, {} unpacked", human_size(n)))
        .unwrap_or_default();

    if looks_binary(&bytes[..bytes.len().min(8192)]) {
        let content = format!(
            "{}<div class=\"bigmsg\"><p>Binary file.</p>{}</div>",
            meta_line(size, mtime, "binary"),
            download_link(rel, vfs, "Download")
                .map(|dl| format!("<p>{dl}</p>"))
                .unwrap_or_default()
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(state, rel, vfs), false, &content);
    }

    let text = String::from_utf8_lossy(&bytes);
    let want_source = query_get(query, "src") == Some("1");

    let source_flag = || {
        flag(
            "",
            &format!("{}?src=1", href_path(rel)),
            &svg_icon(ICON_SOURCE),
            "Source",
            "Highlighted source instead",
        )
    };

    // A table where the file is one, with the source a flag away as Markdown
    // has it. An empty file has no table to draw and falls through to say so.
    let mut table_refused = false;
    if !want_source && TABLE_EXTS.contains(&ext.as_str()) {
        let rows = parse_delimited(&text, delimiter_for(&ext, &text));
        // The same budget as anything else that goes to the page; past it the
        // file is drawn as written, and the meta line says why.
        let drawn = render_table(&rows, MAX_HIGHLIGHT_BYTES as usize);
        table_refused = matches!(drawn, Some(Err(())));
        if let Some(Ok(table)) = drawn {
            let body = rows.len() - 1;
            let extra = match body > MAX_TABLE_ROWS {
                true => format!("{body} rows &middot; first {MAX_TABLE_ROWS} shown"),
                false => format!("{body} row{}", if body == 1 { "" } else { "s" }),
            };
            let controls = format!("{}{}{}", print_flag(state), source_flag(), std_controls(state, rel, vfs));
            let content = format!("{}{table}", meta_line(size, mtime, &format!("{extra}{unpacked}")));
            return layout(state, root, prefs, rel, url_now, &controls, false, &content);
        }
    }

    if !want_source {
        let body = if MARKDOWN_EXTS.contains(&ext.as_str()) {
            Some(render_markdown(
                &state.hl,
                &text,
                !state.cfg.app_ui,
                RawHtml::of(vfs),
            ))
        } else if MERMAID_EXTS.contains(&ext.as_str()) {
            Some(render_mermaid_figure(&text))
        } else {
            None
        };
        if let Some(body) = body {
            let controls = format!(
                "{}{}{}",
                print_flag(state),
                source_flag(),
                std_controls(state, rel, vfs)
            );
            // A rendered page has no meta line; a packed one gets one, to say so.
            let note = match unpacked.is_empty() {
                true => String::new(),
                false => meta_line(size, mtime, unpacked.trim_start_matches(" &middot; ")),
            };
            let content = format!("{note}<div class=\"md\">{}</div>", body);
            return layout(state, root, prefs, rel, url_now, &controls, false, &content);
        }
    }

    // JSON is laid out one member to a line unless the source was asked for —
    // a minified file is one line nobody reads — and as written where it is
    // not JSON after all, or where the layout would be past the limit the
    // highlighter keeps to for any other file.
    let laid_out = match JSON_EXTS.contains(&ext.as_str()) {
        true => reformat_json(&text, MAX_HIGHLIGHT_BYTES as usize),
        false => None,
    };
    let shown = match (&laid_out, want_source) {
        (Some(t), false) => t.as_str(),
        _ => &*text,
    };

    // Highlighted source view.
    let first_line = shown.lines().next().unwrap_or("");
    // JSON by name only where it is JSON: a `.jsonl` that is not stays what
    // its extension and first line make it.
    let syntax = match laid_out.is_some() {
        true => state.hl.syntax_for(name, "json", first_line),
        false => state.hl.syntax_for(name, &ext, first_line),
    };
    let html = state.hl.highlight(syntax, shown);
    let line_count = shown.lines().count().max(1);

    let gutter = if prefs.ln {
        let mut nums = String::with_capacity(line_count * 4);
        for i in 1..=line_count {
            nums.push_str(&i.to_string());
            nums.push('\n');
        }
        format!("<pre class=\"gutter\" aria-hidden=\"true\">{}</pre>", nums)
    } else {
        String::new()
    };

    let mut controls = print_flag(state);
    if laid_out.is_some() && !want_source {
        controls.push_str(&source_flag());
    } else if laid_out.is_some()
        || [MARKDOWN_EXTS, MERMAID_EXTS].iter().any(|exts| exts.contains(&ext.as_str()))
        || (TABLE_EXTS.contains(&ext.as_str()) && !table_refused)
    {
        controls.push_str(&flag(
            "",
            &href_path(rel),
            &svg_icon(ICON_PREVIEW),
            "Preview",
            "Preview instead",
        ));
    }
    controls.push_str(&std_controls(state, rel, vfs));

    let content = format!(
        "{}<div class=\"codewrap\">{}<pre class=\"hl-code\"><code>{}</code></pre></div>",
        meta_line(
            size,
            mtime,
            &format!(
                "{} &middot; {} line{}{unpacked}{}",
                html_escape(&syntax.name),
                line_count,
                if line_count == 1 { "" } else { "s" },
                if table_refused { " &middot; too big to draw as a table" } else { "" }
            )
        ),
        gutter,
        html
    );
    layout(state, root, prefs, rel, url_now, &controls, true, &content)
}
