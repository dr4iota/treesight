use crate::md::{render_markdown, render_mermaid_figure};
use crate::vfs::Vfs;
use crate::page::{
    bare_layout, flag, layout, svg_icon, Prefs, ICON_DOWNLOAD, ICON_PRINT, ICON_RAW, ICON_RENDERED,
    ICON_SOURCE,
};
use crate::util::*;
use crate::vfs::VfsPath;
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
            "Print this page (Ctrl+P)",
        )
    } else {
        String::new()
    }
}

fn std_controls(rel: &[String], vfs: &dyn Vfs) -> String {
    let base = href_path(rel);
    format!(
        "{}{}",
        flag(
            "",
            &format!("{base}?raw=1"),
            &svg_icon(ICON_RAW),
            "Raw",
            "The file as it is on disk"
        ),
        download_flag(&base, vfs)
    )
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
pub fn raw_page(state: &State, root: &Root, prefs: Prefs<'_>, rel: &[String], url_now: &str) -> String {
    let name = rel.last().map(String::as_str).unwrap_or("");
    let base = href_path(rel);
    // The way out is the way in reversed: where every other view offers Raw,
    // this one offers the view it came from. Download stays — it is the other
    // thing wanted of a file one is looking at as a file.
    let controls = format!(
        "{}{}",
        flag(
            "",
            &base,
            &svg_icon(ICON_RENDERED),
            "Rendered",
            "The file as this app shows it"
        ),
        flag(
            "",
            &format!("{base}?dl=1"),
            &svg_icon(ICON_DOWNLOAD),
            "Download",
            "Save a copy"
        )
    );
    let content = format!(
        "<iframe class=\"raw\" src=\"{}\" title=\"{}\"></iframe>",
        html_escape(&raw_href(rel)),
        html_escape(name)
    );
    bare_layout(state, root, prefs, rel, url_now, &controls, &content)
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
        return layout(state, root, prefs, rel, url_now, &std_controls(rel, vfs), false, &content);
    }
    if VIDEO_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<div class=\"fit\"><video class=\"preview\" controls src=\"{}\"></video></div>",
            meta_line(size, mtime, "video"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(rel, vfs), false, &content);
    }
    if AUDIO_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<audio class=\"preview\" controls src=\"{}\"></audio>",
            meta_line(size, mtime, "audio"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(rel, vfs), false, &content);
    }
    if ext == "pdf" {
        let content = format!(
            "{}<div class=\"fit\"><embed class=\"pdf\" src=\"{}\" type=\"application/pdf\"></div>",
            meta_line(size, mtime, "pdf"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(rel, vfs), false, &content);
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
        return layout(state, root, prefs, rel, url_now, &std_controls(rel, vfs), false, &content);
    }

    let Ok(bytes) = vfs.read(canon) else {
        let content = "<div class=\"bigmsg\"><p>Could not read file.</p></div>".to_string();
        return layout(state, root, prefs, rel, url_now, "", false, &content);
    };

    if looks_binary(&bytes[..bytes.len().min(8192)]) {
        let content = format!(
            "{}<div class=\"bigmsg\"><p>Binary file.</p>{}</div>",
            meta_line(size, mtime, "binary"),
            download_link(rel, vfs, "Download")
                .map(|dl| format!("<p>{dl}</p>"))
                .unwrap_or_default()
        );
        return layout(state, root, prefs, rel, url_now, &std_controls(rel, vfs), false, &content);
    }

    let text = String::from_utf8_lossy(&bytes);
    let want_source = query_get(query, "src") == Some("1");

    if !want_source {
        let body = if MARKDOWN_EXTS.contains(&ext.as_str()) {
            Some(render_markdown(&state.hl, &text, !state.cfg.app_ui))
        } else if MERMAID_EXTS.contains(&ext.as_str()) {
            Some(render_mermaid_figure(&text))
        } else {
            None
        };
        if let Some(body) = body {
            let controls = format!(
                "{}{}{}",
                print_flag(state),
                flag(
                    "",
                    &format!("{}?src=1", href_path(rel)),
                    &svg_icon(ICON_SOURCE),
                    "Source",
                    "Highlighted source instead"
                ),
                std_controls(rel, vfs)
            );
            let content = format!("<div class=\"md\">{}</div>", body);
            return layout(state, root, prefs, rel, url_now, &controls, false, &content);
        }
    }

    // Highlighted source view.
    let first_line = text.lines().next().unwrap_or("");
    let syntax = state.hl.syntax_for(name, &ext, first_line);
    let html = state.hl.highlight(syntax, &text);
    let line_count = text.lines().count().max(1);

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
    if MARKDOWN_EXTS.contains(&ext.as_str()) || MERMAID_EXTS.contains(&ext.as_str()) {
        controls.push_str(&flag(
            "",
            &href_path(rel),
            &svg_icon(ICON_RENDERED),
            "Rendered",
            "Rendered view instead",
        ));
    }
    controls.push_str(&std_controls(rel, vfs));

    let content = format!(
        "{}<div class=\"codewrap\">{}<pre class=\"hl-code\"><code>{}</code></pre></div>",
        meta_line(
            size,
            mtime,
            &format!(
                "{} &middot; {} line{}",
                html_escape(&syntax.name),
                line_count,
                if line_count == 1 { "" } else { "s" }
            )
        ),
        gutter,
        html
    );
    layout(state, root, prefs, rel, url_now, &controls, true, &content)
}
