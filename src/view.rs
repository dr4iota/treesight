use crate::data::{delimiter_for, gunzip, parse_delimited, reformat_json, render_table, MAX_TABLE_ROWS};
use crate::md::{render_markdown, render_mermaid_figure, RawHtml};
use crate::vfs::Vfs;
use crate::page::{
    bare_layout, current_flag, dimmed_flag, flag, group, layout, ln_switch, svg_icon, Prefs,
    ICON_DOWNLOAD, ICON_PRINT, ICON_PREVIEW, ICON_RAW, ICON_SOURCE,
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

/// The same, for the Raw view's frame: as text wherever the file is text,
/// HTML included (`serve_raw`), since Raw is the file as written.
fn raw_text_href(rel: &[String]) -> String {
    format!("{}?raw=1&bare=1&as=text", href_path(rel))
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

/// The verbs group: what can be done with the file — Print where the view is a
/// document, Download, and the backend's own (Keep offline). None of them is a
/// view, so none is lit but a verb that is a switch and is on.
fn verbs(state: &State, rel: &[String], vfs: &dyn Vfs, print: bool) -> String {
    let print = match print {
        true => print_flag(state),
        false => String::new(),
    };
    group(&format!(
        "{print}{}{}",
        download_flag(&href_path(rel), vfs),
        file_actions(state, rel, vfs)
    ))
}

/// What a file's page is, as far as its views go: which of Preview, Source
/// and Raw it offers. The rows of the header sketch's table
/// (`docs/mockups/header-buttons.html` in telechore, section 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shown {
    /// Drawn, played or rendered by an element: an image, a film, a sound, a
    /// PDF. Raw would be the same thing in a frame, so there is no group.
    Media,
    /// Text the app draws as well as highlights: Markdown, Mermaid, a table,
    /// JSON laid out, HTML as a page.
    Drawn,
    /// Text it highlights, and nothing more.
    Text,
    /// Text past the highlight limit: Raw, with Source dimmed to say why.
    TooBig,
    /// Not text, or text that only unpacks too big: a notice and Download.
    /// No Raw, which shows a file as it is, and there is no showing these as
    /// they are — a frame pointed at one is blank, or a download.
    Binary,
    /// Nothing to draw at all: past what the backend hands over, or unreadable.
    Nothing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Preview,
    Source,
    Raw,
}

const PREVIEW_TITLE: &str = "The file as this app shows it";
const SOURCE_TITLE: &str = "Highlighted source";

/// The view group: how the file is shown, as one choice with the view on
/// screen lit (`current_flag`), so a change of view changes which segment is
/// lit and nothing else in the header. `lit` is `None` on a notice — too big
/// once unpacked, binary — which is no view.
///
/// `packed` drops Raw from a file the page opened unpacked: Raw is the file
/// as it is, and as it is it is gzip, which there is no showing. Preview and
/// Source show what it holds, and Download hands over the file itself.
fn view_group(shown: Shown, rel: &[String], lit: Option<View>, packed: bool) -> String {
    let base = href_path(rel);
    let seg = |view: View, href: &str, icon: &str, label: &str, title: &str| match lit == Some(view) {
        true => current_flag(&svg_icon(icon), label, title),
        false => flag("", href, &svg_icon(icon), label, title),
    };
    let preview = || seg(View::Preview, &base, ICON_PREVIEW, "Preview", PREVIEW_TITLE);
    // Source is the page itself where there is no Preview, and asked for
    // where there is.
    let source = |href: &str| seg(View::Source, href, ICON_SOURCE, "Source", SOURCE_TITLE);
    let raw = match packed {
        true => String::new(),
        false => seg(View::Raw, &format!("{base}?raw=1"), ICON_RAW, "Raw", "The file as it is on disk"),
    };
    let segments = match shown {
        // Raw is not offered for these, but its address still answers: there it
        // says how to get back.
        Shown::Media if lit == Some(View::Raw) => format!("{}{raw}", preview()),
        Shown::Binary if lit == Some(View::Raw) => raw,
        Shown::Media | Shown::Nothing | Shown::Binary => String::new(),
        Shown::Drawn => format!("{}{}{raw}", preview(), source(&format!("{base}?src=1"))),
        Shown::Text => format!("{}{raw}", source(&base)),
        Shown::TooBig => format!(
            "{}{raw}",
            dimmed_flag(&svg_icon(ICON_SOURCE), "Source", "Too large to highlight")
        ),
    };
    group(&segments)
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
    meta_line_ending(size, mtime, extra, "")
}

/// The same, with a control at its right end: the source view's *Line
/// numbers* (`ln_switch`).
fn meta_line_ending(size: u64, mtime: Option<std::time::SystemTime>, extra: &str, end: &str) -> String {
    format!(
        "<p class=\"filemeta\"><span>{}{} &middot; {}</span>{end}</p>",
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
    raw_view(state, root, prefs, rel, url_now, shown_of(root.vfs.as_ref(), canon, name))
}

/// The raw page for a file already known to be `shown`: from `raw_page`, and
/// from `file_page` for text too big to highlight, which opens here.
///
/// The header is the file page's less Print: the same verbs group, and the
/// same view group with Raw lit, so going to Raw and back changes one segment.
fn raw_view(state: &State, root: &Root, prefs: Prefs<'_>, rel: &[String], url_now: &str, shown: Shown) -> String {
    let name = rel.last().map(String::as_str).unwrap_or("");
    let controls = format!(
        "{}{}",
        verbs(state, rel, root.vfs.as_ref(), false),
        view_group(shown, rel, Some(View::Raw), false)
    );
    // A binary file's Raw is reached only by its address: there is nothing to
    // show as it is, and a frame would fetch all of it to show nothing.
    if shown == Shown::Binary {
        let content = format!(
            "<main><div class=\"bigmsg\"><p>Binary file: nothing to show as text.</p>{}</div></main>",
            download_link(rel, root.vfs.as_ref(), "Download")
                .map(|dl| format!("<p>{dl}</p>"))
                .unwrap_or_default()
        );
        return bare_layout(state, root, prefs, rel, url_now, &controls, &content);
    }
    bare_layout(state, root, prefs, rel, url_now, &controls, &framed(&raw_text_href(rel), name))
}

/// HTML's Preview: the page as a browser draws it, in a frame like Raw's,
/// served as a page that cannot navigate by itself (`as=page`, `serve_page`)
/// where Raw asks for its text. No Print, as on Raw: what is in the frame is
/// the engine's to lay out, not this page's.
fn html_preview(state: &State, root: &Root, prefs: Prefs<'_>, rel: &[String], url_now: &str) -> String {
    let name = rel.last().map(String::as_str).unwrap_or("");
    let controls = format!(
        "{}{}",
        verbs(state, rel, root.vfs.as_ref(), false),
        view_group(Shown::Drawn, rel, Some(View::Preview), false)
    );
    let src = format!("{}?raw=1&bare=1&as=page", href_path(rel));
    bare_layout(state, root, prefs, rel, url_now, &controls, &framed(&src, name))
}

/// The frame Raw and HTML's Preview draw the file in.
///
/// `sandbox` with no allowances: the framed document may be a remote
/// machine's HTML served as itself, and the parent is a page that can reach
/// the shell. `serve_raw` sends CSP `sandbox` on such documents too —
/// attribute and header each cover an engine that misses the other, and both
/// cost nothing.
fn framed(src: &str, name: &str) -> String {
    format!(
        "<iframe class=\"raw\" sandbox src=\"{}\" title=\"{}\"></iframe>",
        html_escape(src),
        html_escape(name)
    )
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

/// What `file_page` makes of this file, for the raw page's view group. The
/// same tests in the same order, reading no more than they need: the 8 KiB
/// the binary test looks at, and the whole file only where whether it has a
/// Preview turns on all of it — a table that may be too wide to draw, JSON
/// that may not parse (`needs_whole`). A `.gz` is never unpacked here: it has
/// no Raw, so its Raw address is the notice whatever it holds.
fn shown_of(vfs: &dyn Vfs, canon: &VfsPath, name: &str) -> Shown {
    let ext = ext_of(name);
    let size = vfs.metadata(canon).map(|m| m.len).unwrap_or(0);
    if vfs.open_limit().is_some_and(|limit| size > limit) {
        return Shown::Nothing;
    }
    if [IMAGE_EXTS, AUDIO_EXTS, VIDEO_EXTS].iter().any(|exts| exts.contains(&ext.as_str())) || ext == "pdf" {
        return Shown::Media;
    }
    if size > MAX_HIGHLIGHT_BYTES {
        return match too_big_is_text(vfs, canon, &ext) {
            Some(true) => Shown::TooBig,
            Some(false) => Shown::Binary,
            None => Shown::Nothing,
        };
    }
    let bytes = match needs_whole(&ext) {
        true => vfs.read(canon).ok(),
        false => head_of(vfs, canon),
    };
    let Some(bytes) = bytes else {
        return Shown::Nothing;
    };
    // A `.gz` that is gzip has nothing to show as it is, whatever it holds —
    // its page shows that unpacked, and offers no Raw. One that is not gzip
    // after all is its own bytes, as it is on the page.
    if ext == "gz" && bytes.starts_with(&[0x1f, 0x8b]) {
        return Shown::Binary;
    }
    if looks_binary(&bytes[..bytes.len().min(8192)]) {
        return Shown::Binary;
    }
    match drawn_as(&ext, &String::from_utf8_lossy(&bytes), false) {
        true => Shown::Drawn,
        false => Shown::Text,
    }
}

/// The first 8 KiB, which is all the binary test reads.
fn head_of(vfs: &dyn Vfs, canon: &VfsPath) -> Option<Vec<u8>> {
    let mut head = Vec::with_capacity(8192);
    vfs.open(canon).ok()?.take(8192).read_to_end(&mut head).ok()?;
    Some(head)
}

/// Whether a file past the highlight limit is text, and so opens in Raw with
/// Source dimmed, rather than a notice. Never a `.gz`, whose Raw is the packed
/// bytes, and never what its head says is binary: a frame cannot draw either,
/// and pointing one at an archive of hundreds of megabytes fetches all of it
/// to show nothing. `None` where the file cannot be read.
fn too_big_is_text(vfs: &dyn Vfs, canon: &VfsPath, ext: &str) -> Option<bool> {
    if ext == "gz" {
        return Some(false);
    }
    head_of(vfs, canon).map(|head| !looks_binary(&head))
}

/// Whether `drawn_as` needs the whole file to answer: a table and JSON, which
/// are drawn only if all of them can be. Markdown, Mermaid and HTML are drawn
/// whatever is in them, so Raw reads their head only.
fn needs_whole(ext: &str) -> bool {
    [TABLE_EXTS, JSON_EXTS].iter().any(|exts| exts.contains(&ext))
}

/// Whether the page draws this text as well as listing it, so has a Preview:
/// Markdown and Mermaid always, HTML unless packed (its Preview is the file
/// served as itself, and a `.gz` is served packed), a table where there is
/// one and it is not too wide, JSON where it parses. The one answer for the
/// file page and Raw, so a change of view never adds or drops a segment.
fn drawn_as(ext: &str, text: &str, packed: bool) -> bool {
    if [MARKDOWN_EXTS, MERMAID_EXTS].iter().any(|exts| exts.contains(&ext)) {
        return true;
    }
    if HTML_EXTS.contains(&ext) {
        return !packed;
    }
    if TABLE_EXTS.contains(&ext) {
        let rows = parse_delimited(text, delimiter_for(ext, text));
        return matches!(render_table(&rows, MAX_HIGHLIGHT_BYTES as usize), Some(Ok(_)));
    }
    JSON_EXTS.contains(&ext) && reformat_json(text, MAX_HIGHLIGHT_BYTES as usize).is_some()
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
        return layout(state, root, prefs, rel, url_now, "", &content);
    }

    if IMAGE_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<div class=\"fit\"><a href=\"{1}\"><img class=\"preview-img\" src=\"{1}\" alt=\"{2}\"></a></div>",
            meta_line(size, mtime, "image"),
            html_escape(&raw_href(rel)),
            html_escape(name)
        );
        return layout(state, root, prefs, rel, url_now, &verbs(state, rel, vfs, false), &content);
    }
    if VIDEO_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<div class=\"fit\"><video class=\"preview\" controls src=\"{}\"></video></div>",
            meta_line(size, mtime, "video"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &verbs(state, rel, vfs, false), &content);
    }
    if AUDIO_EXTS.contains(&ext.as_str()) {
        let content = format!(
            "{}<audio class=\"preview\" controls src=\"{}\"></audio>",
            meta_line(size, mtime, "audio"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &verbs(state, rel, vfs, false), &content);
    }
    if ext == "pdf" {
        let content = format!(
            "{}<div class=\"fit\"><embed class=\"pdf\" src=\"{}\" type=\"application/pdf\"></div>",
            meta_line(size, mtime, "pdf"),
            html_escape(&raw_href(rel))
        );
        return layout(state, root, prefs, rel, url_now, &verbs(state, rel, vfs, false), &content);
    }

    // Text-ish content from here on.
    // Too big to highlight: text goes straight to Raw, with Source in the
    // group dimmed to say why, rather than a page that only points there.
    // Anything else gets the notice and Download, as any binary file does.
    if size > MAX_HIGHLIGHT_BYTES {
        match too_big_is_text(vfs, canon, &ext) {
            Some(true) => return raw_view(state, root, prefs, rel, url_now, Shown::TooBig),
            Some(false) => {
                let content = format!(
                    "{}<div class=\"bigmsg\"><p>File is too large to show here ({}).</p>{}</div>",
                    meta_line(size, mtime, "large file"),
                    human_size(size),
                    download_link(rel, vfs, "Download")
                        .map(|dl| format!("<p>{dl}</p>"))
                        .unwrap_or_default()
                );
                return layout(state, root, prefs, rel, url_now, &verbs(state, rel, vfs, false), &content);
            }
            // Not even its head: nothing past the limit is read whole.
            None => {
                let content = "<div class=\"bigmsg\"><p>Could not read file.</p></div>".to_string();
                return layout(state, root, prefs, rel, url_now, "", &content);
            }
        }
    }

    // HTML's Preview is the page itself, in a frame that fetches the file:
    // the head is all there is to read here, to see that it is text.
    if HTML_EXTS.contains(&ext.as_str()) && query_get(query, "src") != Some("1") {
        match head_of(vfs, canon) {
            Some(head) if !looks_binary(&head) => return html_preview(state, root, prefs, rel, url_now),
            // Binary under an HTML name: the notice, below.
            Some(_) => {}
            None => {
                let content = "<div class=\"bigmsg\"><p>Could not read file.</p></div>".to_string();
                return layout(state, root, prefs, rel, url_now, "", &content);
            }
        }
    }

    let Ok(bytes) = vfs.read(canon) else {
        let content = "<div class=\"bigmsg\"><p>Could not read file.</p></div>".to_string();
        return layout(state, root, prefs, rel, url_now, "", &content);
    };

    // A `.gz` is read as what it holds, under the name it had before it was
    // packed, so `syslog.2.gz` is a log and `rows.csv.gz` a table; Download
    // hands over the packed file, and there is no Raw (`view_group`'s
    // `packed`). One that is not gzip after all
    // falls through as its own bytes, which is the binary notice.
    let (bytes, name, ext, unpacked) = match unpacked_name(name) {
        Some(inner_name) => match gunzip(&bytes[..], MAX_HIGHLIGHT_BYTES) {
            Some((inner, false)) => {
                let len = inner.len() as u64;
                (inner, inner_name, ext_of(inner_name), Some(len))
            }
            // A notice and Download, as for a binary file: Raw here would be
            // the packed bytes, which are no text to show.
            Some((_, true)) => {
                let content = format!(
                    "{}<div class=\"bigmsg\"><p>File is too large to render once unpacked \
                     (over {}).</p>{}</div>",
                    meta_line(size, mtime, "gzip"),
                    human_size(MAX_HIGHLIGHT_BYTES),
                    download_link(rel, vfs, "Download")
                        .map(|dl| format!("<p>{dl}</p>"))
                        .unwrap_or_default()
                );
                return layout(state, root, prefs, rel, url_now, &verbs(state, rel, vfs, false), &content);
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
        return layout(state, root, prefs, rel, url_now, &verbs(state, rel, vfs, false), &content);
    }

    let text = String::from_utf8_lossy(&bytes);
    let want_source = query_get(query, "src") == Some("1");

    // The drawn views below: the verbs, and Preview lit.
    let drawn_controls = || {
        format!(
            "{}{}",
            verbs(state, rel, vfs, true),
            view_group(Shown::Drawn, rel, Some(View::Preview), !unpacked.is_empty())
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
            let controls = drawn_controls();
            let content = format!("{}{table}", meta_line(size, mtime, &format!("{extra}{unpacked}")));
            return layout(state, root, prefs, rel, url_now, &controls, &content);
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
            let controls = drawn_controls();
            // A rendered page has no meta line; a packed one gets one, to say so.
            let note = match unpacked.is_empty() {
                true => String::new(),
                false => meta_line(size, mtime, unpacked.trim_start_matches(" &middot; ")),
            };
            let content = format!("{note}<div class=\"md\">{}</div>", body);
            return layout(state, root, prefs, rel, url_now, &controls, &content);
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

    // Laid-out JSON is this listing and its Preview; Markdown, Mermaid and a
    // table that can be drawn have a Preview elsewhere; the rest is Source. A
    // table asked for as source is asked about too (`drawn_as`), so a table
    // too wide to draw or with nothing in it offers no Preview from here.
    let packed = !unpacked.is_empty();
    let shown_as = match laid_out.is_some() || (!JSON_EXTS.contains(&ext.as_str()) && drawn_as(&ext, &text, packed)) {
        true => Shown::Drawn,
        false => Shown::Text,
    };
    let lit = match laid_out.is_some() && !want_source {
        true => View::Preview,
        false => View::Source,
    };
    let controls = format!(
        "{}{}",
        verbs(state, rel, vfs, true),
        view_group(shown_as, rel, Some(lit), packed)
    );

    let content = format!(
        "{}<div class=\"codewrap\">{}<pre class=\"hl-code\"><code>{}</code></pre></div>",
        meta_line_ending(
            size,
            mtime,
            &format!(
                "{} &middot; {} line{}{unpacked}{}",
                html_escape(&syntax.name),
                line_count,
                if line_count == 1 { "" } else { "s" },
                if table_refused { " &middot; too big to draw as a table" } else { "" }
            ),
            &ln_switch(prefs, url_now)
        ),
        gutter,
        html
    );
    layout(state, root, prefs, rel, url_now, &controls, &content)
}
