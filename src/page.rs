use crate::md::render_markdown;
use crate::util::*;
use crate::vfs::{Vfs, VfsPath};
use crate::{Root, State};

pub use crate::vfs::Entry;

#[derive(Clone, Copy, PartialEq)]
pub enum ThemeMode {
    Auto,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn from_str(s: &str) -> Option<ThemeMode> {
        match s {
            "auto" => Some(ThemeMode::Auto),
            "light" => Some(ThemeMode::Light),
            "dark" => Some(ThemeMode::Dark),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        }
    }
    pub fn next(self) -> ThemeMode {
        match self {
            ThemeMode::Auto => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Auto,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Prefs<'a> {
    pub theme: ThemeMode,
    pub ln: bool,
    pub sidebar: bool,
    /// Directories the reader opened in the pane, `/`-joined relative paths.
    ///
    /// A borrow, so `Prefs` stays `Copy` and every renderer keeps taking it by
    /// value. The chain down to the current directory is deliberately *not* in
    /// here: that one is implied by where you are, and writing it down would mean
    /// a walk through a tree quietly filling the cookie with everywhere you had
    /// been — and would let a collapse hide the row you are standing in.
    pub open: &'a [String],
}

pub fn read_dir_sorted(state: &State, vfs: &dyn Vfs, path: &VfsPath) -> Vec<Entry> {
    let mut out = vfs.read_dir(path).unwrap_or_default();
    if !state.cfg.show_hidden {
        out.retain(|e| !e.name.starts_with('.'));
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

// Icons are drawn, not typed. The obvious characters for these — folder,
// picture, film, note — all live in the emoji planes, and a font stack without
// them (any DejaVu-only Linux, for one) draws a missing-glyph box in their
// place. Paths cannot miss, they take the surrounding colour, and they stay
// legible in both themes. Each constant is the inside of an `<svg>`; `svg_icon`
// supplies the rest.
pub const ICON_FOLDER: &str = "<path d=\"M1.7 4.5c0-.5.4-.9.9-.9h2.9l1.3 1.7h7c.5 0 .9.4.9.9v6.4c0 \
     .5-.4.9-.9.9H2.6a.9.9 0 01-.9-.9V4.5z\"/>";
const ICON_IMAGE: &str = "<path d=\"M2 3.6h12v8.8H2z\"/><path d=\"M2.9 11.6l3.5-3.4 1.9 2 2.3-2.7 \
     3 3.4\"/><path d=\"M5.6 5.6a1.1 1.1 0 100 2.2 1.1 1.1 0 000-2.2z\"/>";
const ICON_AUDIO: &str = "<path d=\"M6.6 11.4V4l6.4-1.4v2.2L6.6 6.2\"/>\
     <path fill=\"currentColor\" stroke=\"none\" d=\"M4.9 9.6a2 2 0 100 4 2 2 0 000-4z\"/>";
const ICON_VIDEO: &str = "<path d=\"M1.9 3.9h12.2v8.2H1.9z\"/>\
     <path fill=\"currentColor\" stroke=\"none\" d=\"M6.6 6.3l4 1.7-4 1.7z\"/>";
const ICON_DOC: &str = "<path d=\"M3.6 2.4h5.6l3.2 3.2v8H3.6z\"/><path d=\"M9.2 2.4v3.2h3.2\"/>\
     <path d=\"M5.6 8.4h5M5.6 10.6h5\"/>";
const ICON_FILE: &str = "<path d=\"M3.6 2.4h5.6l3.2 3.2v8H3.6z\"/><path d=\"M9.2 2.4v3.2h3.2\"/>";
/// The file exactly as it is on disk, so: leaving for it.
pub const ICON_RAW: &str =
    "<path d=\"M9.6 3.4h3v3\"/><path d=\"M12.6 3.4L8.2 7.8\"/><path d=\"M12 9.4v3.2H3.4V4h3.2\"/>";
pub const ICON_DOWNLOAD: &str =
    "<path d=\"M8 2.9v6.7\"/><path d=\"M5.2 7l2.8 2.8L10.8 7\"/><path d=\"M3.2 13.1h9.6\"/>";
pub const ICON_SOURCE: &str = "<path d=\"M6.2 4.4L2.7 8l3.5 3.6\"/><path d=\"M9.8 4.4L13.3 8l-3.5 3.6\"/>";
pub const ICON_RENDERED: &str =
    "<path d=\"M3.4 3h9.2v10H3.4z\"/><path d=\"M5.4 6h5.2M5.4 8.4h5.2M5.4 10.8h3.4\"/>";
/// Refresh: the page as the disk has it now. Three quarters of a circle with the
/// gap and the arrowhead at the top right, so the drawing is the turn itself. The
/// head is the chevron `ICON_UP` and `ICON_BACK` use, rather than the square hook
/// most icon sets put on this one — an arrow is already spelled a particular way
/// here and a second spelling of it would only be a second spelling.
const ICON_REFRESH: &str =
    "<path d=\"M12.16 5.8A4.8 4.8 0 1 1 8 3.4\"/><path d=\"M6.5 2.1L8 3.4l-1.5 1.3\"/>";
/// Print: the sheet going in above, the machine, and the sheet coming out across
/// its front. Three rectangles and no ink drop or wireless wave — the drawing has
/// to survive being 14 pixels wide.
pub const ICON_PRINT: &str = "<path d=\"M4.8 6.2V2.8h6.4v3.4\"/>\
     <path d=\"M4.8 11.4H2.9V6.2h10.2v5.2h-1.9\"/><path d=\"M4.8 9.4h6.4v3.8H4.8z\"/>";
/// The drawer button, which is only ever on screen at a width where the pane
/// is not: three bars, because that is how every narrow window spells the thing
/// that slides in from the left. Tighter and shorter than the three lines
/// `icon_lineno` draws for "off", on purpose — that is the other icon in this
/// header made of horizontal strokes, and at 14 pixels how much of the box they
/// fill is the whole of the difference between them.
const ICON_MENU: &str = "<path d=\"M3.6 5h8.8M3.6 8h8.8M3.6 11h8.8\"/>";

/// The way out of a directory, on the `..` row.
const ICON_UP: &str = "<path d=\"M8 12.8V3.8\"/><path d=\"M4.3 7.5L8 3.8l3.7 3.7\"/>";
/// The shell's Back button. Drawn like the rest rather than typed as `←`, which
/// is a character with its own advance width and its own idea of the baseline,
/// and so never lined up with the icons beside it.
const ICON_BACK: &str = "<path d=\"M13 8H3\"/><path d=\"M7.2 3.8L3 8l4.2 4.2\"/>";
/// The theme flag says which setting is chosen rather than which one is next: a
/// sun for light, a moon for dark, and half of each for following the system.
/// Three settings, three drawings — resolving `auto` to the sun or the moon the
/// system happens to be on would draw it as an explicit choice, and lose the one
/// thing about that setting worth showing.
const ICON_SUN: &str = "<path d=\"M8 4.8a3.2 3.2 0 100 6.4 3.2 3.2 0 000-6.4z\"/>\
     <path d=\"M8 1.2v1.6M8 13.2v1.6M1.2 8h1.6M13.2 8h1.6M3.2 3.2l1.2 1.2\
     M11.6 11.6l1.2 1.2M12.8 3.2l-1.2 1.2M4.4 11.6l-1.2 1.2\"/>";
/// Centred on (8, 8) with a radius of 6, so it sits where the sun sits.
const ICON_MOON: &str = "<path d=\"M14 8.53A6 6 0 117.47 2 4.67 4.67 0 0014 8.53z\"/>";
/// Half filled, on the same circle. The fill runs out to the middle of the
/// stroke for the reason the pane's does: stopping at the inside of it leaves a
/// hairline seam. Unlike `Ln` and `Tree`, this fill is not a state — the flag has
/// three settings, so half-and-half means half of each, not "on".
const ICON_THEME_AUTO: &str = "<path d=\"M8 2a6 6 0 100 12A6 6 0 008 2z\"/>\
     <path fill=\"currentColor\" stroke=\"none\" d=\"M8 2a6 6 0 000 12z\"/>";
// The two switches below are binary, so each says which way it is set in its own
// ink — more ink for on — since that is the whole of the state at the widths
// where the words are gone. How the ink arrives differs, and it follows the
// thing being switched rather than one house rule: the pane is there either way
// and fills in, while the line numbers are simply drawn or not.
//
// Neither draws anything on top of a fill, which is deliberate: an outline
// showing through solid ink would have to be painted in the colour behind the
// pill to read as a hole, and the ink here is `currentColor` — `--muted`, a
// mid-grey that goes *lighter* in the dark theme. So the knockout could not be
// white; it would have to be `--bg-subtle` and track the theme with it. Filling
// in place of the detail rather than over it avoids owning that problem, and at
// 14px a 1px hole in a 3px strip only reads as mud anyway.

/// The tree flag: a pane down the left of the window, which is what it hides.
fn icon_pane(on: bool) -> String {
    format!(
        "<path d=\"M2.2 3h11.6v10H2.2z\"/><path d=\"M6.4 3v10\"/>{}",
        if on {
            // Run the fill out to the middle of the frame and of the divider
            // rather than stopping at the inside of either stroke: butting two
            // shapes of the same colour edge to edge leaves a hairline of
            // half-covered pixels between them.
            "<path fill=\"currentColor\" stroke=\"none\" d=\"M2.2 3h4.2v10H2.2z\"/>"
        } else {
            "<path d=\"M3.6 5.8h1.4M3.6 8h1.4\"/>"
        }
    )
}

/// The action a pane section's heading can carry. A plus and nothing else: the
/// bar is .75rem of uppercase and there is room for one mark on the end of it,
/// so what it draws is the one thing a list of things can be asked for.
pub const ICON_PLUS: &str = "<path d=\"M8 3.6v8.8M3.6 8h8.8\"/>";

/// "Forget this row": a cross, the mark that has meant *remove* on a list since
/// lists had rows. Drawn to `ICON_PLUS`'s measurements, which is what puts it on
/// the same optical weight as the one action a heading can carry.
const ICON_CROSS: &str = "<path d=\"M4.5 4.5l7 7M11.5 4.5l-7 7\"/>";
// A ribbon, outlined for a root that is not pinned and filled for one that is.
// Two states of one shape rather than two shapes: the control stays in the same
// place and says which way round it is, which a second icon would not.
const ICON_PIN: &str = "<path d=\"M4.2 2.6h7.6v10.8L8 10.6l-3.8 2.8z\"/>";
const ICON_PINNED: &str =
    "<path fill=\"currentColor\" d=\"M4.2 2.6h7.6v10.8L8 10.6l-3.8 2.8z\"/>";

/// "Serve this folder as the root": an arrow going in through the side of a frame.
const ICON_AS_ROOT: &str = "<path d=\"M9.8 3.4h2.8v9.2H9.8\"/><path d=\"M3.4 8h5.8\"/>\
     <path d=\"M6.8 5.6L9.2 8l-2.4 2.4\"/>";

/// Line numbers: the lines they count, with the numbers beside them when they are
/// on. Presence, not fill, for this one — the marks in the gutter *are* the
/// numbers, so drawing them is what "on" means and anything else reads backwards.
/// The lines run the full width once the numbers are gone, which is what the
/// gutter's space does on the page too.
fn icon_lineno(on: bool) -> &'static str {
    if on {
        "<path d=\"M2.6 4.2h.8M2.6 8h.8M2.6 11.8h.8\"/>\
         <path d=\"M6 4.2h7.4M6 8h7.4M6 11.8h7.4\"/>"
    } else {
        "<path d=\"M2.6 4.2h10.8M2.6 8h10.8M2.6 11.8h10.8\"/>"
    }
}

/// Wraps icon paths in an `<svg>` that inherits colour and text size.
/// The theme flag's mark and the words under it: which setting is chosen, and
/// what clicking does next.
///
/// Public because the flag is not only in this header any more — a shell that
/// serves pages of its own beside the tree puts the same control on them, and
/// three places showing one setting should be three places drawing one mark.
pub fn theme_icon(mode: ThemeMode) -> (&'static str, &'static str) {
    match mode {
        ThemeMode::Auto => (
            ICON_THEME_AUTO,
            "Theme: following the system — click for light",
        ),
        ThemeMode::Light => (ICON_SUN, "Theme: light — click for dark"),
        ThemeMode::Dark => (ICON_MOON, "Theme: dark — click to follow the system"),
    }
}

pub fn svg_icon(paths: &str) -> String {
    format!(
        "<svg viewBox=\"0 0 16 16\" width=\"14\" height=\"14\" fill=\"none\" stroke=\"currentColor\" \
         stroke-width=\"1.2\" stroke-linecap=\"round\" stroke-linejoin=\"round\" \
         aria-hidden=\"true\">{paths}</svg>"
    )
}

/// The icon cell of a listing row.
///
/// Directories carry the class rather than being found by one: `tr:has(a.dir)`
/// asked the row about its link, which left the colour to whether the webview
/// knew `:has()`, and never applied to search results, whose links carry no
/// class at all.
fn entry_icon(name: &str, is_dir: bool) -> String {
    icon_cell(is_dir, icon_for(name, is_dir))
}

fn icon_cell(is_dir: bool, paths: &str) -> String {
    format!(
        "<span class=\"icon{}\">{}</span>",
        if is_dir { " dir" } else { "" },
        svg_icon(paths)
    )
}

fn icon_for(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return ICON_FOLDER;
    }
    let ext = ext_of(name);
    if IMAGE_EXTS.contains(&ext.as_str()) {
        ICON_IMAGE
    } else if AUDIO_EXTS.contains(&ext.as_str()) {
        ICON_AUDIO
    } else if VIDEO_EXTS.contains(&ext.as_str()) {
        ICON_VIDEO
    } else if MARKDOWN_EXTS.contains(&ext.as_str()) || MERMAID_EXTS.contains(&ext.as_str()) {
        ICON_DOC
    } else {
        ICON_FILE
    }
}

fn set_href(key: &str, val: &str, back: &str) -> String {
    format!("/.ts/set?{}={}&back={}", key, val, percent_encode(back))
}

/// A control that spells itself out when there is room and shrinks to a symbol
/// when there is not. `icon` is raw markup, and always a drawn one: every
/// character that would do instead brings its own metrics, and a row of pills is
/// only tidy when the thing inside each one measures the same.
pub(crate) fn flag(class: &str, href: &str, icon: &str, label: &str, title: &str) -> String {
    let class = if class.is_empty() {
        String::new()
    } else {
        format!(" class=\"{class}\"")
    };
    format!(
        "<a{} href=\"{}\" title=\"{}\"><span class=\"ico\">{}</span><span class=\"lbl\">{}</span></a>",
        class,
        html_escape(href),
        html_escape(title),
        icon,
        html_escape(label)
    )
}

/// Everything down to the end of the header: the head, and the one line that
/// says where you are and what can be done here. Both page shapes below start
/// with it and differ only in what they hang underneath.
#[allow(clippy::too_many_arguments)]
fn head_and_header(
    state: &State,
    root: &Root,
    prefs: Prefs<'_>,
    rel: &[String],
    url_now: &str,
    extra_controls: &str,
    show_ln_toggle: bool,
    show_pane_flag: bool,
    extra_body_class: &str,
) -> String {
    let site_title = state.cfg.title_for(root);
    // The document is titled by *name* where there is one — a bookmark's label,
    // say — and by the folder otherwise. The header says the rest: the folder in
    // the crumbs, the machine in the tag beside them, neither of which a window
    // title has room for.
    let named = state.cfg.root_name().unwrap_or_else(|| site_title.clone());
    let title = if rel.is_empty() {
        named
    } else {
        format!("{} — {}", rel.join("/"), named)
    };

    let data_theme = match prefs.theme {
        ThemeMode::Auto => String::new(),
        m => format!(" data-theme=\"{}\"", m.as_str()),
    };
    // The syntax colours live in a stylesheet of their own, which is out of reach
    // of the print rules in ours: a dark theme would send its own pale code to
    // paper and the page would print with a gap where the listing was. So paper
    // is named in the light sheet's media and screens in the dark one's — the
    // same swap the palette makes for everything else, made where it has to be.
    let syntax_css = match prefs.theme {
        ThemeMode::Auto => concat!(
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-light.css\" media=\"print, (prefers-color-scheme: light)\">",
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-dark.css\" media=\"screen and (prefers-color-scheme: dark)\">"
        )
        .to_string(),
        ThemeMode::Light => "<link rel=\"stylesheet\" href=\"/.ts/syntax-light.css\">".to_string(),
        ThemeMode::Dark => concat!(
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-dark.css\" media=\"screen\">",
            "<link rel=\"stylesheet\" href=\"/.ts/syntax-light.css\" media=\"print\">"
        )
        .to_string(),
    };

    // Breadcrumbs, behind the id of the machine they are on when they are not on
    // this one: `/home/hanhua` is a path every host in the world has a version of,
    // and the listing should not be the only thing in the window that knows which.
    //
    // Its own element beside the crumbs rather than the first thing inside them,
    // because on a narrow window the crumbs take a row to themselves and the badge
    // should not go with them: which machine you are on belongs on the line with
    // the buttons, and the row below is the path's to fill.
    let tag = match crate::root_id_bookmark(&root.id) {
        Some(id) => format!("<span class=\"tag\">{}</span>", html_escape(id)),
        None => String::new(),
    };
    let mut crumbs = String::new();
    crumbs.push_str(&format!("<a href=\"/\">{}</a>", html_escape(&site_title)));
    let mut acc: Vec<String> = Vec::new();
    for (i, seg) in rel.iter().enumerate() {
        acc.push(seg.clone());
        crumbs.push_str("<span class=\"sep\">/</span>");
        if i + 1 == rel.len() {
            crumbs.push_str(&html_escape(seg));
        } else {
            crumbs.push_str(&format!(
                "<a href=\"{}/\">{}</a>",
                html_escape(&href_path(&acc)),
                html_escape(seg)
            ));
        }
    }

    // One control for both halves of the window, because there is only one
    // window: the pane and the listing come out of the same request, and the tree
    // re-reads its directories every time it is drawn. It goes ahead of the
    // page's own flags so that it sits in the same place on a listing, a file and
    // an error, instead of shifting along by however many controls that page
    // happens to bring.
    //
    // Shell only. There it is a link the shell turns into an actual reload, and
    // the window it is in has no other way to ask for one — the app has no
    // address bar and no reload button of its own. A browser has both, sitting
    // directly above ours and doing the same thing better, so on the web this is
    // a second button for something the reader already has.
    //
    // Each title spells out both the state and what a click does, because from
    // the width where the words go it is the only thing left that can.
    let mut controls = String::new();
    if state.cfg.app_ui {
        controls.push_str(&flag(
            "",
            "/.ts/reload",
            &svg_icon(ICON_REFRESH),
            "Refresh",
            "Reload this page (F5)",
        ));
    }
    controls.push_str(extra_controls);
    if show_ln_toggle {
        let (label, val) = if prefs.ln { ("Ln: on", "0") } else { ("Ln: off", "1") };
        controls.push_str(&flag(
            "",
            &set_href("ln", val, url_now),
            &svg_icon(&icon_lineno(prefs.ln)),
            label,
            if prefs.ln {
                "Line numbers on — click to hide"
            } else {
                "Line numbers off — click to show"
            },
        ));
    }
    // The switch is the pane, not the tree inside it: with the pane on, the tree
    // is what it is for and is always there, and with the pane off the listing
    // has the window. A switch for the tree alone would have left an empty
    // column behind, which is neither of the two things anyone wants.
    let (pane_label, pane_val, pane_title) = if prefs.sidebar {
        ("Pane: on", "0", "Side pane shown — click to hide")
    } else {
        ("Pane: off", "1", "Side pane hidden — click to show")
    };
    // Not with the flags on the right: the switch and the drawer button are one
    // control in one place, at the left end of the row. Which of the two is on
    // screen is the stylesheet's to say, and it says it by width — the switch
    // while the pane is a column beside the listing, the button once it can only
    // be a drawer over it. Two elements because they are two mechanisms: the
    // switch is a link that stores a preference, and the drawer is a checkbox
    // that must not navigate at all, which is what makes it work on a page with
    // no script in it. See `paneflag` in `app.css`.
    let pane_flag = match show_pane_flag {
        true => format!(
            "\n  {}",
            flag(
                "paneflag",
                &set_href("sidebar", pane_val, url_now),
                &svg_icon(&icon_pane(prefs.sidebar)),
                pane_label,
                pane_title,
            )
        ),
        // The raw view: there is no pane on that page for this to be the switch
        // of, at any width.
        false => String::new(),
    };
    let (mark, why) = theme_icon(prefs.theme);
    controls.push_str(&flag(
        "",
        &set_href("theme", prefs.theme.next().as_str(), url_now),
        &svg_icon(mark),
        &format!("Theme: {}", prefs.theme.as_str()),
        why,
    ));

    // The shell's own chrome, and all of it: one button on the line the path and
    // the flags already had, rather than a browser-style row of its own. Both
    // links are inert here — the shell intercepts them, and this server has no
    // route for either.
    let back = if state.cfg.app_ui {
        format!(
            "\n  {}",
            flag("back", "/.ts/back", &svg_icon(ICON_BACK), "Back", "Back (Alt+Left)")
        )
    } else {
        String::new()
    };
    // The pane at a width where there is no room for it: the same markup,
    // slid over the listing by a checkbox nothing but CSS reads. It rides with
    // the pane rather than with the pages that could have one — with the pane
    // switched off there is no tree, no Places and no Recent, so a button that
    // opened a drawer onto them would open a drawer onto nothing.
    //
    // The checkbox goes ahead of everything as a sibling of `.shell`, which is
    // the whole trick: `#ts-drawer:checked ~ .shell nav.tree` is how a page
    // with no script in it remembers that a button was pressed. The label may
    // sit anywhere, and does — in the header, where the buttons are.
    // Not `&& prefs.sidebar`: the switch above is a wide-window control — the
    // stylesheet takes it away at the width where the pane becomes a drawer, and
    // puts this in its place — so at drawer widths it says whatever it last said
    // on a wider window, and letting it decide meant a phone with no way to
    // reach the tree at all.
    let drawer = show_pane_flag;
    let drawer_toggle = if drawer {
        "<input type=\"checkbox\" id=\"ts-drawer\" class=\"drawer-toggle\" aria-hidden=\"true\">\n"
    } else {
        ""
    };
    let drawer_btn = if drawer {
        format!(
            "\n  <label for=\"ts-drawer\" class=\"drawer-btn\" title=\"Tree and places\">{}</label>",
            svg_icon(ICON_MENU)
        )
    } else {
        String::new()
    };

    // `nopane` rather than leaving the markup out: see the pane above.
    let off = match prefs.sidebar {
        true => "",
        false => " nopane",
    };
    let classes = match (state.cfg.app_ui, extra_body_class) {
        (true, "") => format!(" class=\"app{off}\""),
        (true, c) => format!(" class=\"app {c}{off}\""),
        (false, "") if off.is_empty() => String::new(),
        (false, "") => format!(" class=\"{}\"", off.trim()),
        (false, c) => format!(" class=\"{c}{off}\""),
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en"{data_theme}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content">
<title>{title}</title>
<link rel="icon" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='%234c8dff' stroke-width='1.4' stroke-linejoin='round'><path d='M1.7 4.5c0-.5.4-.9.9-.9h2.9l1.3 1.7h7c.5 0 .9.4.9.9v6.4c0 .5-.4.9-.9.9H2.6a.9.9 0 01-.9-.9V4.5z'/></svg>">
<link rel="stylesheet" href="/.ts/app.css">
<link rel="stylesheet" href="/.ts/math.css">
{syntax_css}
</head>
<body{classes}>
{drawer_toggle}<header>{back}{drawer_btn}{pane_flag}{tag}
  <div class="crumbs">{crumbs}</div>
  <div class="controls">{controls}</div>
</header>"#,
        data_theme = data_theme,
        title = html_escape(&title),
        syntax_css = syntax_css,
        classes = classes,
        drawer_toggle = drawer_toggle,
        back = back,
        drawer_btn = drawer_btn,
        pane_flag = pane_flag,
        tag = tag,
        crumbs = crumbs,
        controls = controls,
    )
}

/// A page with no root behind it: the start page, and the wait page on the way
/// to a first folder.
///
/// Its own skeleton rather than `layout` with everything switched off. What the
/// header carries — crumbs, the pane flag, line numbers, the path in the footer —
/// is all about a root, and a header full of controls for a folder nobody has
/// chosen is chrome pretending there is something to do with it. What stays is
/// the name, the theme, and the way in.
fn rootless_page(state: &State, prefs: Prefs<'_>, url_now: &str, content: &str) -> String {
    let mut controls = String::new();
    let (mark, why) = theme_icon(prefs.theme);
    controls.push_str(&flag(
        "",
        &set_href("theme", prefs.theme.next().as_str(), url_now),
        &svg_icon(mark),
        &format!("Theme: {}", prefs.theme.as_str()),
        why,
    ));
    let data_theme = match prefs.theme {
        ThemeMode::Auto => String::new(),
        m => format!(" data-theme=\"{}\"", m.as_str()),
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en"{data_theme}>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content">
<title>{title}</title>
<link rel="icon" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='%234c8dff' stroke-width='1.4' stroke-linejoin='round'><path d='M1.7 4.5c0-.5.4-.9.9-.9h2.9l1.3 1.7h7c.5 0 .9.4.9.9v6.4c0 .5-.4.9-.9.9H2.6a.9.9 0 01-.9-.9V4.5z'/></svg>">
<link rel="stylesheet" href="/.ts/app.css">
</head>
<body class="app nothing">
<header>
  <div class="crumbs"></div>
  <div class="controls">{controls}</div>
</header>
<main>
{content}
</main>
<footer><span class="where"><span class="app">{app}</span></span></footer>
</body>
</html>
"#,
        data_theme = data_theme,
        title = html_escape(&state.cfg.title()),
        controls = controls,
        content = content,
        app = html_escape(&state.cfg.app_label()),
    )
}

/// Full page shell. `rel` is the current path segments, `url_now` the raw
/// (still percent-encoded) path+query of this request, used for toggles.
pub fn layout(
    state: &State,
    root: &Root,
    prefs: Prefs<'_>,
    rel: &[String],
    url_now: &str,
    extra_controls: &str,
    show_ln_toggle: bool,
    content: &str,
) -> String {
    // The picker lives in the status line, which is always on screen — the pane
    // that used to hold it is the first thing to go when the window narrows. Only
    // where there is one to open, though, and that is the embedder's answer
    // rather than the platform's: a phone that has wired a per-directory grant to
    // `/.ts/open` sets the flag and gets the button, and one that has not does
    // not, because a button that cannot do the one thing it says is worse than
    // no button.
    let pick = if state.cfg.app_ui && state.cfg.picker {
        flag(
            "pick",
            "/.ts/open",
            &svg_icon(ICON_FOLDER),
            "Open Folder…",
            "Open Folder… (Ctrl+O)",
        )
    } else {
        String::new()
    };

    // One switch, one thing: the pane is either there with everything in it or
    // not there at all. In the shell that takes Places and Recent with it, which
    // is the honest trade for a full-width listing — the picker they are
    // shortcuts to is in the status line, and that line is always on screen.
    // Rendered whether or not the switch is on, and hidden by a class when it is
    // off. The drawer needs something to slide in, and only the stylesheet knows
    // whether this window is wide enough for the switch to have meant anything.
    let sidebar = pane_html(state, root, rel, prefs, url_now);

    // What closes the drawer: the listing, made into the label of the same
    // checkbox for as long as the drawer is over it. Only where the pane is,
    // since it is only ever the pane it closes.
    let scrim = "<label for=\"ts-drawer\" class=\"drawer-scrim\" aria-hidden=\"true\"></label>\n";

    format!(
        r#"{chrome}
<div class="shell">
{scrim}{sidebar}
<main>
{content}
</main>
</div>
<footer>{footer}</footer>
</body>
</html>
"#,
        chrome = head_and_header(
            state,
            root,
            prefs,
            rel,
            url_now,
            extra_controls,
            show_ln_toggle,
            true,
            ""
        ),
        scrim = scrim,
        sidebar = sidebar,
        content = content,
        // The served root gets the same head-and-leaf treatment as a Recent, and
        // for the same reason: what a plain ellipsis drops off the end of a path
        // is the folder you are actually in. The version goes first in the line
        // and last in importance, so it is what leaves when the line is short.
        footer = format!(
            "<span class=\"where\" title=\"{0}\"><span class=\"app\">{1} &middot;</span>{2}</span>\
             {3}",
            html_escape(&root.id),
            html_escape(&state.cfg.app_label()),
            path_label(&root.id),
            pick
        ),
    )
}

/// The raw view's page: the line that says which file this is, and under it the
/// file. Nothing else — no pane, no status line, not a pixel of padding of ours
/// — because what is under that line is not our document to lay out. Whatever
/// margins it has are the ones it brought, and the engine showing it is the one
/// that knows what they should be.
pub fn bare_layout(
    state: &State,
    root: &Root,
    prefs: Prefs<'_>,
    rel: &[String],
    url_now: &str,
    extra_controls: &str,
    content: &str,
) -> String {
    format!(
        "{chrome}\n{content}\n</body>\n</html>\n",
        chrome = head_and_header(
            state,
            root,
            prefs,
            rel,
            url_now,
            extra_controls,
            false,
            false,
            "rawview"
        ),
        content = content,
    )
}

const TREE_MAX_PER_DIR: usize = 150;

/// The left pane: the directory tree, and in the desktop shell the Places and
/// Recent shortcuts below it plus the folder picker.
///
/// Only called when the pane is on, and then it always has its tree — the flag
/// in the header is the pane itself, all of it or none.
fn pane_html(
    state: &State,
    root: &Root,
    cur: &[String],
    prefs: Prefs<'_>,
    url_now: &str,
) -> String {
    let mut out = String::from("<nav class=\"tree\">");
    // The tree first and foremost: it is what the pane is for, and it is what
    // grows, so it takes the height and the shortcuts below settle for what is
    // left. Under a heading like the lists below it, because it is one more of
    // them — the difference being that this one is somewhere you are, so the
    // heading carries the way to be somewhere else instead.
    // Both ends of having a folder open, on the heading of the thing that is open:
    // another one, and none. They go together in a group so the bar stays a line
    // with two ends — a heading at one and its controls at the other — rather than
    // spreading three children evenly across it.
    let mut acts: Vec<(String, String, String)> = Vec::new();
    // Pinning acts on the *root*, and this is the heading of the root — which is
    // why it moved here from the window's controls. There it sat among Back,
    // Reload and Print, every one of which acts on the page you are looking at,
    // and read as though it pinned that: walk three directories in and the
    // control still meant the folder you opened. On the heading of what is open
    // it cannot mean anything else.
    //
    // To pin a folder further down, make it the root first — every directory row
    // in the tree carries the button for that.
    if state.cfg.app_ui {
        let (href, icon, title) = match state.cfg.is_pinned(&root.id) {
            true => ("/.ts/unpin", ICON_PINNED, "Pinned — click to remove"),
            false => ("/.ts/pin", ICON_PIN, "Pin this folder"),
        };
        acts.push((href.to_string(), icon.to_string(), title.to_string()));
    }
    // The picker is not here any more: it is in the status line, which is always
    // on screen, and closing this folder puts the start page in front of you with
    // every way in on it. Two of the same button on one screen, and this was the
    // one that could be spared.
    if state.cfg.app_ui {
        acts.push((
            "/.ts/close".to_string(),
            ICON_CROSS.to_string(),
            "Close this folder".to_string(),
        ));
    }
    out.push_str(&format!(
        "<section class=\"files\"><h2>Files{}</h2>",
        heading_acts(&acts)
    ));
    tree_dir(
        state,
        root.vfs.as_ref(),
        &mut Vec::new(),
        cur,
        prefs,
        url_now,
        &mut out,
    );
    out.push_str("</section>");
    if state.cfg.app_ui {
        out.push_str("<div class=\"chooser\">");
        // Two paths on purpose: opening a Place is not something Recent should
        // collect, or the fixed list would keep copying itself into the other
        // one. Opening something from Recent does move it back to the top.
        root_list(
            &mut out,
            state,
            "places",
            "Places",
            &[],
            state.cfg.places.iter().map(|(l, p)| Row {
                label: Some(l),
                id: p,
                action: "/.ts/place",
                aside: &[],
            }),
        );
        // What the reader chose to keep, between the list the platform decides and the
        // one that collects itself. Each row carries its own way out, in the aside slot
        // Recent and an embedder's sections already use.
        let pinned = state.cfg.pinned();
        let unpin: Vec<[(String, String, String); 1]> = pinned
            .iter()
            .map(|p| {
                [(
                    format!("/.ts/unpin?path={}", percent_encode(&p.id)),
                    ICON_CROSS.to_string(),
                    "Unpin this folder".to_string(),
                )]
            })
            .collect();
        root_list(
            &mut out,
            state,
            "pinned",
            "Pinned",
            &[],
            pinned.iter().zip(&unpin).map(|(p, aside)| Row {
                label: p.label.as_deref(),
                id: &p.id,
                action: "/.ts/place",
                aside,
            }),
        );
        // Whatever the embedder brought, between the fixed list and the
        // remembered one — a list of servers is a Places somebody else knows
        // the contents of, so it belongs where Places is and not under Recent.
        for sec in state.cfg.sections().iter() {
            root_list(
                &mut out,
                state,
                &sec.class,
                &sec.heading,
                &sec.heading_acts,
                sec.entries.iter().map(|e| Row {
                    label: e.label.as_deref(),
                    id: &e.id,
                    action: &e.action,
                    aside: &e.aside,
                }),
            );
        }
        // No label, so each of these is drawn as its path: a Place is somewhere
        // with a name, a Recent is just a folder you were in, and which one it
        // was is a question about where it sits.
        //
        // The one list here that is a record rather than a fixture, so the one
        // that can hold something the reader wants gone — a folder that moved, a
        // root a since-fixed bug wrote down wrong. Each row carries its own way
        // out, in the aside slot an embedder's sections already use.
        let recent = state.cfg.recent();
        let forget: Vec<[(String, String, String); 1]> = recent
            .iter()
            .map(|p| {
                [(
                    format!("/.ts/forget?path={}", percent_encode(p)),
                    ICON_CROSS.to_string(),
                    "Forget this folder".to_string(),
                )]
            })
            .collect();
        root_list(
            &mut out,
            state,
            "recent",
            "Recent",
            &[],
            recent.iter().zip(&forget).map(|(p, aside)| Row {
                label: None,
                id: p,
                action: "/.ts/root",
                aside,
            }),
        );
        out.push_str("</div>");
    }
    out.push_str("</nav>");
    out
}

/// One row of a pane section as the renderer needs it, borrowed from whatever
/// holds it: a fixed Places pair, a Recent id, or a [`crate::PaneEntry`]. The
/// `action` rides on the row rather than on the section, because a section the
/// embedder brought can send each of its entries somewhere of its own.
struct Row<'a> {
    label: Option<&'a str>,
    id: &'a str,
    action: &'a str,
    aside: &'a [(String, String, String)],
}

/// One pane section of "serve this root instead" links. `action` is the path the
/// shell recognises, which is also how it tells a Place from a Recent. An item
/// with no label is drawn as its id, by `path_label`.
///
/// An entry whose check has come back badly is greyed and says what happened,
/// and stays a link: the check is a snapshot from whenever it ran, a drive that
/// was not ready can be ready now, and the only way to find out is to ask for
/// it. Clicking one costs whatever the wait costs, which is the same wait the
/// list used to charge everybody up front.
/// The controls on a section heading, grouped at the far end of the bar.
///
/// One renderer for every heading that has any: a section the embedder brought,
/// and the Files heading, which carries Pin and Close. The wrapper goes on even
/// for a single control — `h2:has(a.secact)` is what turns the bar into a line
/// with two ends, and `.acts` is what keeps several of them together at one of
/// them.
fn heading_acts(acts: &[(String, String, String)]) -> String {
    if acts.is_empty() {
        return String::new();
    }
    let links: String = acts
        .iter()
        .map(|(href, icon, title)| {
            format!(
                "<a class=\"secact\" href=\"{}\" title=\"{}\">{}</a>",
                html_escape(href),
                html_escape(title),
                svg_icon(icon)
            )
        })
        .collect();
    format!("<span class=\"acts\">{links}</span>")
}

fn root_list<'a, I: Iterator<Item = Row<'a>>>(
    out: &mut String,
    state: &State,
    class: &str,
    heading: &str,
    acts: &[(String, String, String)],
    items: I,
) {
    let links: String = items
        .map(|row| {
            let note = state.cfg.root_status(row.id).note();
            let link = format!(
                "<a href=\"{}?path={}\" title=\"{}\">{}</a>",
                row.action,
                percent_encode(row.id),
                html_escape(row.id),
                match row.label {
                    Some(l) => html_escape(l),
                    None => path_label(row.id),
                }
            );
            format!(
                "<li{}>{}{}</li>",
                if note.is_some() { " class=\"gone\"" } else { "" },
                if row.aside.is_empty() {
                    link
                } else {
                    // The tree's row wrapper, for the reason the tree has it:
                    // the entry and its buttons share a line of their own, so a
                    // long name ellipsises against the buttons instead of
                    // pushing them off the pane.
                    format!(
                        "<span class=\"row\">{}{}</span>",
                        link,
                        row.aside
                            .iter()
                            .map(|(href, icon, title)| format!(
                                "<a class=\"aside\" href=\"{}\" title=\"{}\">{}</a>",
                                html_escape(href),
                                html_escape(title),
                                svg_icon(icon)
                            ))
                            .collect::<String>()
                    )
                },
                match note {
                    Some(n) => format!("<span class=\"why\">{n}</span>"),
                    None => String::new(),
                }
            )
        })
        .collect();
    if links.is_empty() {
        return;
    }
    out.push_str(&format!(
        "<section class=\"{}\"><h2>{}{}</h2><ul>{}</ul></section>",
        html_escape(class),
        html_escape(heading),
        heading_acts(acts),
        links
    ));
}

/// A remembered root split into the path above it and the folder itself, so the
/// pane can show as many levels as it has room for and drop the middle of the
/// rest. The name is the part that identifies the entry, so it is the part that
/// never goes: the stylesheet ellipsises the head and leaves the leaf alone.
///
/// Splitting here rather than measuring anywhere is what keeps this a static
/// page. How much of `/home/hanhua/mix` survives is a question about the width
/// of a rendered box, and CSS is the only thing on either side of the wire that
/// knows that — the server would have to guess at a font it cannot see.
fn path_label(full: &str) -> String {
    match full.rfind(|c| c == '/' || c == '\\') {
        // A separator with something after it. The separator goes to the *leaf*,
        // not the end of the head: it is the one character that says the name is
        // a name under something, and on the head it would be the first thing an
        // ellipsis ate — leaving `/home/hanhua/w…project-alpha`, where the name
        // reads as the rest of the clipped word. On the leaf it cannot be lost:
        // `/home/hanhua/w…/project-alpha`. Nothing changes when the whole path
        // fits, since the two halves still spell it exactly.
        Some(i) if i + 1 < full.len() => format!(
            "{}<span class=\"leaf\">{}</span>",
            // Empty for an absolute path of one component — `/hanhua` is all
            // leaf, and a head of nothing is a box with an ellipsis in it.
            match &full[..i] {
                "" => String::new(),
                head => format!("<span class=\"head\">{}</span>", html_escape(head)),
            },
            html_escape(&full[i..])
        ),
        // A trailing separator, or none at all: a filesystem or drive root, or a
        // lone name. Either way there is no path above it to shorten.
        _ => format!("<span class=\"leaf\">{}</span>", html_escape(full)),
    }
}

/// The "serve this folder instead" button on a directory row in the tree.
///
/// Clicking the name walks into a directory; this re-roots to it, which is the
/// thing you cannot otherwise do without the picker and a path you have already
/// got on screen. It goes through `/.ts/root`, the same link a Recent uses, so the
/// shell remembers it in Recent exactly as it would any other opened root.
///
/// Only in the shell. Nothing else can act on it: the server has no such route,
/// and a page served over a network has no business offering one.
fn as_root_link(state: &State, vfs: &dyn Vfs, path: &VfsPath) -> String {
    if !state.cfg.app_ui {
        return String::new();
    }
    let full = vfs.root_id_at(path);
    format!(
        "<a class=\"asroot\" href=\"/.ts/root?path={}\" title=\"Serve {} as the root\">{}</a>",
        percent_encode(&full),
        html_escape(&full),
        svg_icon(ICON_AS_ROOT)
    )
}

/// The disclosure arrow.
///
/// It used to be a character inside the name's own link, which is why clicking it
/// walked into the directory: there was no second control there to click. Now it
/// is its own link to `/.ts/tree`, and it says which way it means to go — a link
/// that toggles is a link that is wrong when it is followed twice.
///
/// A directory on the way to the current one has no link at all. It is open
/// because you are standing in it, and an arrow that could shut it would hide
/// where you are.
fn twisty(open: bool, on_path: bool, key: &str, back: &str) -> String {
    const DOWN: &str = "&#x25BE;";
    const RIGHT: &str = "&#x25B8;";
    if on_path {
        return format!("<span class=\"twisty\">{DOWN}</span>");
    }
    let (param, mark, what) = match open {
        true => ("shut", DOWN, "Collapse"),
        false => ("open", RIGHT, "Expand"),
    };
    format!(
        "<a class=\"twisty\" href=\"/.ts/tree?{}={}&amp;back={}\" title=\"{} {}\">{}</a>",
        param,
        percent_encode(key),
        percent_encode(back),
        what,
        html_escape(key),
        mark
    )
}

fn tree_dir(
    state: &State,
    vfs: &dyn Vfs,
    rel: &mut Vec<String>,
    cur: &[String],
    prefs: Prefs<'_>,
    url_now: &str,
    out: &mut String,
) {
    let entries = read_dir_sorted(state, vfs, &VfsPath::new(rel.clone()));
    let total = entries.len();
    // An opened directory with nothing in it drew `<ul></ul>`: nothing to see, and
    // a pair of tags to wonder about in the markup. The read has already happened
    // by here, so knowing it is empty costs nothing.
    if total == 0 {
        return;
    }
    out.push_str("<ul>");
    for e in entries.into_iter().take(TREE_MAX_PER_DIR) {
        rel.push(e.name.clone());
        let is_cur = rel.as_slice() == cur;
        let on_path = cur.len() >= rel.len() && cur[..rel.len()] == rel[..];
        let href = href_path(rel);
        let cls = if is_cur { " class=\"cur\"" } else { "" };
        if e.is_dir {
            let key = rel.join("/");
            // Two sources, and the row is open if either says so: the chain to
            // where you are, and the set you opened by hand.
            let open = on_path || prefs.open.iter().any(|p| p == &key);
            let child = VfsPath::new(rel.clone());
            // The name and the button share a row of their own, so an expanded
            // directory's children hang below it rather than beside the button,
            // and a long name ellipsises against the button instead of pushing it
            // off the pane.
            out.push_str(&format!(
                "<li{}><span class=\"row\">{}<a class=\"dir\" href=\"{}/\">{}/</a>{}</span>",
                cls,
                twisty(open, on_path, &key, url_now),
                html_escape(&href),
                html_escape(&e.name),
                as_root_link(state, vfs, &child)
            ));
            if open {
                tree_dir(state, vfs, rel, cur, prefs, url_now, out);
            }
            out.push_str("</li>");
        } else {
            out.push_str(&format!(
                "<li{}><a href=\"{}\">{}</a></li>",
                cls,
                html_escape(&href),
                html_escape(&e.name)
            ));
        }
        rel.pop();
    }
    if total > TREE_MAX_PER_DIR {
        out.push_str(&format!(
            "<li class=\"more\">&hellip; {} more</li>",
            total - TREE_MAX_PER_DIR
        ));
    }
    out.push_str("</ul>");
}

const SEARCH_MAX_RESULTS: usize = 2000;
const SEARCH_MAX_SCANNED: usize = 50_000;
const SEARCH_MAX_DEPTH: usize = 12;

pub fn listing_page(
    state: &State,
    root: &Root,
    prefs: Prefs<'_>,
    rel: &[String],
    canon: &VfsPath,
    query: &[(String, String)],
    url_now: &str,
) -> String {
    let vfs = root.vfs.as_ref();
    let q = query_get(query, "q").unwrap_or("");
    let recursive = query_get(query, "r") == Some("1");

    let mut content = format!(
        r#"<form class="filter" method="get">
<input type="text" name="q" value="{}" placeholder="glob, e.g. *.rs" aria-label="filter pattern">
<label><input type="checkbox" name="r" value="1"{}> recursive</label>
<button>Filter</button>{}
</form>
"#,
        html_escape(q),
        if recursive { " checked" } else { "" },
        if q.is_empty() {
            String::new()
        } else {
            format!(
                " <a href=\"{}\">clear</a>",
                html_escape(&href_path(rel).trim_end_matches('/').to_string() /* dir href */) + "/"
            )
        }
    );

    if q.is_empty() {
        content.push_str(&entries_table(state, vfs, rel, canon));
        content.push_str(&listing_readme(state, vfs, canon));
    } else {
        content.push_str(&search_results(state, vfs, rel, canon, q, recursive));
    }

    layout(state, root, prefs, rel, url_now, "", false, &content)
}

const README_NAMES: &[&str] = &["README.md", "README.markdown", "README.mdown", "README.mkd"];

/// GitHub-style README under an unfiltered listing. Missing, huge, or binary
/// files are skipped so the table still stands on its own.
fn listing_readme(state: &State, vfs: &dyn Vfs, dir: &VfsPath) -> String {
    for name in README_NAMES {
        let path = dir.join(name);
        let Ok(meta) = vfs.metadata(&path) else {
            continue;
        };
        if !meta.is_file || meta.len > MAX_HIGHLIGHT_BYTES {
            continue;
        }
        let Ok(bytes) = vfs.read(&path) else {
            continue;
        };
        if looks_binary(&bytes[..bytes.len().min(8192)]) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        return format!(
            "<section class=\"listing-readme\"><div class=\"md\">{}</div></section>",
            render_markdown(&state.hl, &text)
        );
    }
    String::new()
}

fn entries_table(state: &State, vfs: &dyn Vfs, rel: &[String], canon: &VfsPath) -> String {
    let entries = read_dir_sorted(state, vfs, canon);
    let mut rows = String::new();
    if !rel.is_empty() {
        let parent = &rel[..rel.len() - 1];
        rows.push_str(&format!(
            "<tr><td>{}<a href=\"{}/\">..</a></td><td class=\"size\"></td><td class=\"time\"></td></tr>",
            icon_cell(true, ICON_UP),
            html_escape(href_path(parent).trim_end_matches('/'))
        ));
    }
    for e in &entries {
        let mut href = {
            let mut r = rel.to_vec();
            r.push(e.name.clone());
            href_path(&r)
        };
        if e.is_dir {
            href.push('/');
        }
        rows.push_str(&format!(
            "<tr><td>{}<a href=\"{}\"{}>{}</a></td><td class=\"size\">{}</td><td class=\"time\">{}</td></tr>",
            entry_icon(&e.name, e.is_dir),
            html_escape(&href),
            if e.is_dir { " class=\"dir\"" } else { "" },
            html_escape(&e.name),
            if e.is_dir {
                "&mdash;".to_string()
            } else {
                human_size(e.size)
            },
            e.mtime.map(fmt_time).unwrap_or_default(),
        ));
    }
    if entries.is_empty() {
        rows.push_str("<tr><td colspan=\"3\"><em>empty directory</em></td></tr>");
    }
    format!(
        "<table class=\"listing\"><tr><th>Name</th><th class=\"size\">Size</th><th class=\"time\">Modified</th></tr>{}</table>",
        rows
    )
}

fn search_results(
    state: &State,
    vfs: &dyn Vfs,
    rel: &[String],
    canon: &VfsPath,
    pat: &str,
    recursive: bool,
) -> String {
    // Pattern containing '/' matches the path relative to this directory;
    // otherwise it matches the file name only.
    let match_path = pat.contains('/');
    let mut results: Vec<(Vec<String>, Entry)> = Vec::new();
    let mut scanned = 0usize;
    let mut truncated = false;

    // DFS; non-recursive mode just doesn't descend.
    let mut stack: Vec<(VfsPath, Vec<String>)> = vec![(canon.clone(), Vec::new())];
    while let Some((dir, drel)) = stack.pop() {
        for e in read_dir_sorted(state, vfs, &dir) {
            scanned += 1;
            if scanned > SEARCH_MAX_SCANNED || results.len() >= SEARCH_MAX_RESULTS {
                truncated = true;
                break;
            }
            let mut erel = drel.clone();
            erel.push(e.name.clone());
            let hay = if match_path {
                erel.join("/")
            } else {
                e.name.clone()
            };
            if fnmatch(pat, &hay) {
                results.push((erel.clone(), e));
            } else if e.is_dir && recursive && erel.len() < SEARCH_MAX_DEPTH {
                stack.push((dir.join(erel.last().unwrap()), erel));
            } else if e.is_dir && recursive {
                truncated = true;
            }
        }
        if truncated {
            break;
        }
    }

    let mut out = format!(
        "<p class=\"matchnote\">{} match{}{}{}</p>",
        results.len(),
        if results.len() == 1 { "" } else { "es" },
        if recursive { " (recursive)" } else { "" },
        if truncated { ", truncated" } else { "" }
    );
    let mut rows = String::new();
    for (erel, e) in &results {
        let mut full = rel.to_vec();
        full.extend(erel.iter().cloned());
        let mut href = href_path(&full);
        if e.is_dir {
            href.push('/');
        }
        rows.push_str(&format!(
            "<tr><td>{}<a href=\"{}\">{}</a></td><td class=\"size\">{}</td><td class=\"time\">{}</td></tr>",
            entry_icon(&e.name, e.is_dir),
            html_escape(&href),
            html_escape(&erel.join("/")),
            if e.is_dir {
                "&mdash;".to_string()
            } else {
                human_size(e.size)
            },
            e.mtime.map(fmt_time).unwrap_or_default(),
        ));
    }
    if !results.is_empty() {
        out.push_str(&format!(
            "<table class=\"listing\"><tr><th>Path</th><th class=\"size\">Size</th><th class=\"time\">Modified</th></tr>{}</table>",
            rows
        ));
    }
    out
}

/// Plain-text listing for non-browser clients (curl, scripts).
pub fn listing_text(state: &State, vfs: &dyn Vfs, path: &VfsPath) -> String {
    let mut out = String::new();
    for e in read_dir_sorted(state, vfs, path) {
        out.push_str(&e.name);
        if e.is_dir {
            out.push('/');
        }
        out.push('\n');
    }
    out
}

/// Shown while the shell is finding out whether a folder can be opened.
///
/// Resolving a path is a syscall with no time limit — a drive letter mapped to a
/// host that is off takes as long as the network stack takes to give up — so the
/// shell does it on a thread and parks the window here meanwhile. Still the served
/// root's page furniture, because that is still what is being served: only the
/// middle of the window is waiting.
pub fn wait_page(
    state: &State,
    root: Option<&Root>,
    prefs: Prefs<'_>,
    url_now: &str,
    path: &str,
) -> String {
    let content = format!(
        "<div class=\"bigmsg\"><p>Opening {}&hellip;</p>\
         <p>If the folder is on a drive or a share that is not answering, this waits \
         for as long as that takes to find out.</p></div>",
        html_escape(path)
    );
    // The first folder of the session is opened from the start page, where there
    // is no root yet — so this page has to be drawable without one.
    match root {
        Some(root) => layout(state, root, prefs, &[], url_now, "", false, &content),
        None => rootless_page(state, prefs, url_now, &content),
    }
}

/// What there is to open, when nothing is.
///
/// The lists are the pane's, in the pane's own markup and out of the same
/// renderer — a start page that drew its own version of Places would be a second
/// version to keep in step. What it does not have is the pane: a sidebar of
/// shortcuts beside a page of the same shortcuts is the same list twice, and the
/// pane earns its place once there is a tree in it.
pub fn start_page(state: &State, prefs: Prefs<'_>, url_now: &str) -> String {
    let mut lists = String::from("<div class=\"start\">");
    root_list(
        &mut lists,
        state,
        "places",
        "Places",
        &[],
        state.cfg.places.iter().map(|(l, p)| Row {
            label: Some(l),
            id: p,
            action: "/.ts/place",
            aside: &[],
        }),
    );
    // What the reader chose to keep, between the list the platform decides and the
    // one that collects itself. Each row carries its own way out, in the aside slot
    // Recent and an embedder's sections already use.
    let pinned = state.cfg.pinned();
    let unpin: Vec<[(String, String, String); 1]> = pinned
        .iter()
        .map(|p| {
            [(
                format!("/.ts/unpin?path={}", percent_encode(&p.id)),
                ICON_CROSS.to_string(),
                "Unpin this folder".to_string(),
            )]
        })
        .collect();
    root_list(
        &mut lists,
        state,
        "pinned",
        "Pinned",
        &[],
        pinned.iter().zip(&unpin).map(|(p, aside)| Row {
            label: p.label.as_deref(),
            id: &p.id,
            action: "/.ts/place",
            aside,
        }),
    );
    for sec in state.cfg.sections().iter() {
        root_list(
            &mut lists,
            state,
            &sec.class,
            &sec.heading,
            &sec.heading_acts,
            sec.entries.iter().map(|e| Row {
                label: e.label.as_deref(),
                id: &e.id,
                action: &e.action,
                aside: &e.aside,
            }),
        );
    }
    let recent = state.cfg.recent();
    let forget: Vec<[(String, String, String); 1]> = recent
        .iter()
        .map(|p| {
            [(
                format!("/.ts/forget?path={}", percent_encode(p)),
                ICON_CROSS.to_string(),
                "Forget this folder".to_string(),
            )]
        })
        .collect();
    root_list(
        &mut lists,
        state,
        "recent",
        "Recent",
        &[],
        recent.iter().zip(&forget).map(|(p, aside)| Row {
            label: None,
            id: p,
            action: "/.ts/root",
            aside,
        }),
    );
    lists.push_str("</div>");

    let said = state.cfg.intro.as_deref().unwrap_or(
        "Browse a folder as a tree: files beside their contents, and nothing \
         installed on whatever machine you are reading from.",
    );
    let intro = format!(
        "<div class=\"intro\"><h1>{name}</h1><p>{}</p>{}</div>",
        html_escape(said),
        match state.cfg.picker {
            true => format!(
                "<p><a class=\"open\" href=\"/.ts/open\">{} Open a folder&hellip;</a></p>",
                svg_icon(ICON_FOLDER)
            ),
            // A platform whose folder picker we cannot ask: the lists are the way
            // in, and saying so beats offering a button that does nothing.
            false => "<p class=\"hint\">Open one of the places below to start.</p>".to_string(),
        },
        name = html_escape(&state.cfg.title()),
    );
    rootless_page(state, prefs, url_now, &format!("{intro}{lists}"))
}

pub fn error_page(
    state: &State,
    root: &Root,
    prefs: Prefs<'_>,
    rel: &[String],
    url_now: &str,
    code: u32,
    msg: &str,
) -> String {
    let content = format!(
        "<div class=\"bigmsg\"><h2>{}</h2><p>{}</p><p><a href=\"/\">Back to root</a></p></div>",
        code,
        html_escape(msg)
    );
    layout(state, root, prefs, rel, url_now, "", false, &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hl::Hl;
    use crate::{Config, PaneEntry, PaneSection, RootStatus};
    use std::fs;
    use std::path::PathBuf;

    fn prefs() -> Prefs<'static> {
        Prefs {
            theme: ThemeMode::Light,
            ln: false,
            sidebar: false,
            open: &[],
        }
    }

    fn state_at(root: PathBuf) -> State {
        State {
            cfg: Config::new(root),
            hl: Hl::for_tests(),
        }
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "treeserve-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The arrow is a control of its own, and it says which way it goes. The row
    /// you are standing in has none: it is open because you are in it.
    #[test]
    fn an_arrow_opens_a_directory_without_walking_into_it() {
        let dir = tmp_dir("twisty");
        fs::create_dir_all(dir.join("here/inner")).unwrap();
        fs::create_dir_all(dir.join("other/deep")).unwrap();
        let state = state_at(dir.clone());
        let cur = vec!["here".to_string()];
        let shut = Prefs { sidebar: true, ..prefs() };
        let page = |prefs: Prefs<'_>| {
            let root = state.cfg.root().expect("these tests always serve one");
            listing_page(&state, &root, prefs, &cur, &VfsPath::new(cur.clone()), &[], "/here/")
        };

        let html = page(shut);
        // The name is the walk-in link, and the arrow is no longer inside it.
        assert!(html.contains("<a class=\"dir\" href=\"/other/\">other/</a>"), "{html}");
        // A closed row offers to open, and says where to come back to.
        assert!(
            html.contains("<a class=\"twisty\" href=\"/.ts/tree?open=other&amp;back=%2Fhere%2F\""),
            "{html}"
        );
        // Nothing under it is drawn until it is open.
        assert!(!html.contains(">deep/</a>"), "{html}");
        // The current directory is open with no control on it, and its child is
        // drawn — that is the implicit chain, which no cookie carries.
        assert!(html.contains("<span class=\"twisty\">"), "{html}");
        assert!(html.contains(">inner/</a>"), "{html}");

        // Opened by hand: the children appear, and the arrow now offers to shut.
        let open = [String::from("other")];
        let html = page(Prefs { open: &open, ..shut });
        assert!(html.contains(">deep/</a>"), "{html}");
        assert!(
            html.contains("<a class=\"twisty\" href=\"/.ts/tree?shut=other&amp;back=%2Fhere%2F\""),
            "{html}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    /// The page a window opens on when nothing has been opened in it: what there
    /// is to open, and no tree — the pane's lists are the page here, and a pane
    /// beside them would be the same list twice.
    ///
    /// Rootless on purpose, and with no temporary directory: this is the one page
    /// that reads nothing off a disk, and `handle` only reaches it when there is
    /// no root to read.
    #[test]
    fn the_start_page_offers_what_there_is_to_open() {
        let mut state = State {
            cfg: Config::rootless(),
            hl: Hl::for_tests(),
        };
        state.cfg.app_ui = true;
        state.cfg.picker = true;
        state.cfg.places = vec![("Home".to_string(), "/home/x".to_string())];
        state.cfg.set_recent(vec!["ssh:iota:/home/hanhua".to_string()]);
        state.cfg.set_sections(vec![PaneSection {
            class: "servers".to_string(),
            heading: "Servers".to_string(),
            heading_acts: vec![(
                "/x/manage".to_string(),
                ICON_PLUS.to_string(),
                "Manage".to_string(),
            )],
            entries: vec![PaneEntry {
                label: Some("Iota".to_string()),
                id: "ssh:iota:~".to_string(),
                action: "/x/open".to_string(),
                aside: Vec::new(),
            }],
        }]);
        let html = start_page(&state, prefs(), "/");

        assert!(html.contains("<body class=\"app nothing\">"), "{html}");
        assert!(!html.contains("<nav class=\"tree\">"), "no pane on this page");
        assert!(html.contains("href=\"/.ts/open\""), "the way in");
        for want in ["Places", "Servers", "Recent", "Home", "Iota", "/x/manage"] {
            assert!(html.contains(want), "missing {want} in {html}");
        }
        // The lists are the pane's rows, so a Recent still offers to be forgotten.
        assert!(html.contains("/.ts/forget?path="), "{html}");

        // What the page calls itself, and where. The name belongs to whoever
        // embedded this crate — a regression here is a window titled after the
        // wrong program, which is the kind of thing that goes unnoticed.
        state.cfg.app_name = Some("downstream".to_string());
        state.cfg.app_version = Some("9.9.9".to_string());
        state.cfg.intro = Some("A sentence of <its> own.".to_string());
        let html = start_page(&state, prefs(), "/");
        assert!(html.contains("<h1>downstream</h1>"), "{html}");
        assert!(html.contains("<title>downstream</title>"), "{html}");
        // Said once: the header's crumbs are empty on this page, on purpose.
        assert!(html.contains("<div class=\"crumbs\"></div>"), "{html}");
        assert!(html.contains("downstream v9.9.9"), "footer names the app");
        assert!(!html.contains("treeserve v"), "not this crate's name");
        // The embedder's sentence, escaped: it is text, not markup.
        assert!(html.contains("A sentence of &lt;its&gt; own."), "{html}");

        // No picker on this platform: say so instead of drawing a dead button.
        state.cfg.picker = false;
        let html = start_page(&state, prefs(), "/");
        assert!(!html.contains("href=\"/.ts/open\""), "{html}");
        assert!(html.contains("Open one of the places below"), "{html}");
    }

    /// What the heading of the open root offers: pinning it, and closing it. The
    /// way *in* is not here — the picker is in the status line, which survives the
    /// pane being switched off, and having it in both places was one button too
    /// many on a narrow window.
    #[test]
    fn the_files_heading_offers_a_pin_and_a_way_out() {
        let dir = tmp_dir("files-acts");
        let mut state = state_at(dir.clone());
        let prefs = Prefs { sidebar: true, ..prefs() };
        let page = |state: &State| {
            let root = state.cfg.root().expect("these tests always serve one");
            listing_page(state, &root, prefs, &[], &VfsPath::root(), &[], "/")
        };

        // A server with no shell around it has neither: the root is the one it was
        // started on, and there is nothing here that could open or close one.
        let html = page(&state);
        assert!(!html.contains("/.ts/open"), "{html}");
        assert!(!html.contains("/.ts/close"), "{html}");

        state.cfg.app_ui = true;
        let html = page(&state);
        assert!(html.contains("href=\"/.ts/close\""), "a way out without a picker");
        assert!(html.contains("href=\"/.ts/pin\""), "and a way to keep it");
        assert!(!html.contains("href=\"/.ts/open\""), "no picker on this platform");

        // With a picker, it is a flag in the status line and not an act on this
        // heading — the same link, drawn once.
        state.cfg.picker = true;
        let html = page(&state);
        assert!(html.contains("class=\"pick\" href=\"/.ts/open\""), "{html}");
        assert!(
            !html.contains("class=\"secact\" href=\"/.ts/open\""),
            "the picker left the heading: {html}"
        );
        assert!(html.contains("href=\"/.ts/close\""), "{html}");

        fs::remove_dir_all(&dir).unwrap();
    }

    /// A remote listing says which machine it is from. `/home/hanhua` is a path
    /// every host has a version of, and the pane's Servers list only says which
    /// one you *clicked*, not which one you are looking at now.
    #[test]
    fn a_remote_root_wears_its_id_beside_the_path() {
        let dir = tmp_dir("rootid");
        let mut state = state_at(dir.clone());
        state.cfg.app_ui = true;
        let prefs = Prefs { sidebar: true, ..prefs() };
        let page = |state: &State| {
            let root = state.cfg.root().expect("these tests always serve one");
            listing_page(state, &root, prefs, &[], &VfsPath::root(), &[], "/")
        };

        // A local root has no id to show, and shows none.
        assert!(!page(&state).contains("class=\"tag\""), "local root");

        let local = state.cfg.root().expect("these tests always serve one");
        state.cfg.set_root_vfs(Root {
            id: "ssh:iota:/home/hanhua".to_string(),
            vfs: std::sync::Arc::clone(&local.vfs),
        });
        let html = page(&state);
        // Beside the crumbs, not inside them: the badge stays on the row with the
        // buttons when a narrow window sends the path to a row of its own.
        assert!(
            html.contains("<span class=\"tag\">iota</span>\n  <div class=\"crumbs\"><a href=\"/\">"),
            "{html}"
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    /// A bar with six controls on it needs the words off sooner than a bar with
    /// three, so the count picks the width rather than one width serving both.
    ///
    /// Words or marks is one answer for the whole row: the bands say when it
    /// flips and set it in one place, and everything wearing a pill reads it
    /// from there. Half a band — words off without marks on — is a row of empty
    /// buttons, which is what the first cut of this did.
    #[test]
    fn a_crowded_header_drops_its_words_sooner() {
        let sheet = crate::app_css();
        // Who reads the answer. Without these the bands set variables nothing
        // consults, and every control keeps its words at every width.
        assert!(sheet.contains(".lbl { display: var(--lbl, inline); }"), "{sheet}");
        assert!(sheet.contains(".ico { display: var(--ico, none); }"), "{sheet}");
        // The base band, and the two the count reaches for above it.
        for (width, selector) in [
            (46, "body".to_string()),
            (56, "body:has(header .controls > :nth-child(4))".to_string()),
            (68, "body:has(header .controls > :nth-child(6))".to_string()),
        ] {
            let at = format!("@media (max-width: {width}rem) {{");
            let from = sheet.find(&at).unwrap_or_else(|| panic!("no {at}"));
            let block = &sheet[from..from + sheet[from..].find("\n}").unwrap_or(0)];
            let rule = block
                .find(&format!("{selector} {{"))
                .map(|i| &block[i..])
                .unwrap_or_else(|| panic!("{width}rem: no {selector}"));
            for decl in ["--lbl: none;", "--ico: flex;", "--pill: 1.7rem;"] {
                assert!(rule.contains(decl), "{width}rem: no {decl}");
            }
        }
    }

    /// The pane switch and the drawer button are one control in one place: the
    /// switch while the pane can be a column, the button once it can only slide
    /// over the listing. So the switch is not among the flags on the right — it
    /// sits at the left end with Back, where the button will replace it — and
    /// the stylesheet is what takes one away at the pane's own width.
    #[test]
    fn the_pane_switch_stands_where_the_drawer_button_will() {
        let dir = tmp_dir("paneswitch");
        let mut state = state_at(dir.clone());
        state.cfg.app_ui = true;
        let root = state.cfg.root().expect("these tests always serve one");
        let html = listing_page(&state, &root, prefs(), &[], &VfsPath::root(), &[], "/");

        // On the left, beside the button that stands in for it.
        assert!(html.contains("class=\"drawer-btn\""), "{html}");
        let switch = html.find("class=\"paneflag\"").expect("a pane switch");
        let controls = html.find("<div class=\"controls\">").expect("the flags");
        assert!(switch < controls, "the switch left the right-hand group: {html}");

        // And the stylesheet swaps the two at the width where the pane goes.
        let sheet = crate::app_css();
        let at = sheet
            .find("@media (max-width: 50rem) {")
            .expect("the pane's own width");
        let block = &sheet[at..at + sheet[at..].find("\n}").unwrap_or(0)];
        assert!(block.contains(".paneflag { display: none; }"), "{block}");

        fs::remove_dir_all(&dir).unwrap();
    }

    /// The pinned list is the reader's own, so its rows unpin — and the control
    /// that puts a root in it sits on the heading of that root, saying which way
    /// round it is. Not in the window's controls: there it read as though it
    /// pinned the page, which is not what it does.
    #[test]
    fn pinning_is_an_act_on_the_files_heading_and_the_rows_undo_it() {
        let dir = tmp_dir("pinned");
        let mut state = state_at(dir.clone());
        state.cfg.app_ui = true;
        let prefs = Prefs { sidebar: true, ..prefs() };
        let page = |state: &State| {
            let root = state.cfg.root().expect("these tests always serve one");
            listing_page(state, &root, prefs, &[], &VfsPath::root(), &[], "/")
        };

        // Nothing pinned: the control offers to, from the heading of what is
        // open, and no list is drawn.
        let html = page(&state);
        assert!(
            html.contains("<span class=\"acts\"><a class=\"secact\" href=\"/.ts/pin\""),
            "{html}"
        );
        assert!(!html.contains("/.ts/unpin"), "{html}");
        assert!(!html.contains(">Pinned<"), "{html}");

        // The open root, pinned: the same control now takes it back.
        let id = state.cfg.root().expect("served").id.clone();
        state.cfg.set_pinned(vec![crate::Pin {
            id: id.clone(),
            label: Some("Home of it all".to_string()),
        }]);
        let html = page(&state);
        assert!(
            html.contains("<span class=\"acts\"><a class=\"secact\" href=\"/.ts/unpin\""),
            "{html}"
        );
        assert!(!html.contains("href=\"/.ts/pin\""), "{html}");
        // Under the name it was pinned with, and with a row of its own to undo.
        assert!(html.contains(">Home of it all</a>"), "{html}");
        assert!(
            html.contains(&format!(
                "<a class=\"aside\" href=\"/.ts/unpin?path={}\" title=\"Unpin this folder\">",
                percent_encode(&id)
            )),
            "{html}"
        );

        // Something else pinned: the control is back to offering.
        state.cfg.set_pinned(vec![crate::Pin {
            id: "ssh:iota:/var/www".to_string(),
            label: None,
        }]);
        let html = page(&state);
        assert!(html.contains("href=\"/.ts/pin\""), "{html}");

        fs::remove_dir_all(&dir).unwrap();
    }

    /// Every Recent row carries its own way out of the list, and only Recent:
    /// Places is a fixture the reader did not write and cannot unwrite here.
    #[test]
    fn a_recent_row_has_a_forget_button_and_a_place_does_not() {
        let dir = tmp_dir("forget");
        let mut state = state_at(dir.clone());
        state.cfg.app_ui = true;
        state.cfg.places = vec![("Home".to_string(), "/home/x".to_string())];
        state.cfg.set_recent(vec!["ssh:iota:/home/hanhua/~".to_string()]);
        let prefs = Prefs { sidebar: true, ..prefs() };
        let root = state.cfg.root().expect("these tests always serve one");
        let html = listing_page(&state, &root, prefs, &[], &VfsPath::root(), &[], "/");

        let button = concat!(
            "<a class=\"aside\" href=\"/.ts/forget?path=ssh%3Aiota%3A%2Fhome%2Fhanhua%2F~\"",
            " title=\"Forget this folder\">"
        );
        assert!(html.contains(button), "{html}");
        // The Places row is drawn the plain way: no row wrapper, no button.
        let place = concat!(
            "<li><a href=\"/.ts/place?path=%2Fhome%2Fx\"",
            " title=\"/home/x\">Home</a></li>"
        );
        assert!(html.contains(place), "{html}");
        assert_eq!(html.matches("/.ts/forget").count(), 1, "{html}");

        fs::remove_dir_all(&dir).unwrap();
    }

    /// A section the embedder brought is drawn where Places and Recent are
    /// drawn, out of the same parts: an entry that links somewhere the shell
    /// knows, a note when its check came back badly, and — this being the part
    /// Places has no use for — buttons of its own on the row and one on the
    /// heading. And it is shell-only furniture, so a browser gets none of it.
    #[test]
    fn a_pane_section_is_drawn_like_places_with_buttons() {
        let dir = tmp_dir("sections");
        let mut state = state_at(dir.clone());
        let id = "ssh:prod-web:/var/www";
        state.cfg.set_sections(vec![PaneSection {
            class: "servers".to_string(),
            heading: "Servers".to_string(),
            // Two, in the order given: a heading bar takes as many as the
            // embedder has, which is what lets a list be both added to and
            // managed without one mark standing for both.
            heading_acts: vec![
                (
                    "/x/add".to_string(),
                    ICON_PLUS.to_string(),
                    "Add a server".to_string(),
                ),
                (
                    "/x/manage".to_string(),
                    ICON_CROSS.to_string(),
                    "Manage servers".to_string(),
                ),
            ],
            entries: vec![PaneEntry {
                label: Some("prod-web".to_string()),
                id: id.to_string(),
                action: "/x/open".to_string(),
                aside: vec![(
                    "/x/edit?id=prod-web".to_string(),
                    ICON_PLUS.to_string(),
                    "Edit prod-web".to_string(),
                )],
            }],
        }]);
        state.cfg.set_root_status(id.to_string(), RootStatus::Missing);
        // The pane is what the section is in, and the test helper has it off.
        let prefs = Prefs { sidebar: true, ..prefs() };

        let page = |state: &State| {
            let root = state.cfg.root().expect("these tests always serve one");
            listing_page(state, &root, prefs, &[], &VfsPath::root(), &[], "/")
        };

        // Nothing outside the shell, whatever the section says.
        let browser = page(&state);
        assert!(!browser.contains("servers"), "{browser}");

        state.cfg.app_ui = true;
        let html = page(&state);
        assert!(
            html.contains("<section class=\"servers\"><h2>Servers<span class=\"acts\">\
                 <a class=\"secact\" href=\"/x/add\" title=\"Add a server\">"),
            "{html}"
        );
        assert!(
            html.contains("title=\"Add a server\"><svg"),
            "the first act keeps its mark: {html}"
        );
        assert!(
            html.contains("</a><a class=\"secact\" href=\"/x/manage\" title=\"Manage servers\">"),
            "and the second follows it inside the one wrapper: {html}"
        );
        assert!(
            html.contains(
                "<li class=\"gone\"><span class=\"row\"><a href=\"/x/open?path=\
                 ssh%3Aprod-web%3A%2Fvar%2Fwww\" title=\"ssh:prod-web:/var/www\">prod-web</a>"
            ),
            "{html}"
        );
        assert!(
            html.contains("<a class=\"aside\" href=\"/x/edit?id=prod-web\" title=\"Edit prod-web\">"),
            "{html}"
        );
        assert!(html.contains("<span class=\"why\">missing</span>"), "{html}");
        // Between the fixed list and the remembered one, not after both.
        state.cfg.places = vec![("Home".to_string(), "/home/x".to_string())];
        state.cfg.set_recent(vec!["/tmp".to_string()]);
        let ordered = page(&state);
        let at = |needle: &str| ordered.find(needle).unwrap_or_else(|| panic!("{ordered}"));
        assert!(at("\"places\"") < at("\"servers\"") && at("\"servers\"") < at("\"recent\""));

        // And a section that has nothing in it yet says nothing.
        state.cfg.set_sections(vec![PaneSection {
            class: "servers".to_string(),
            heading: "Servers".to_string(),
            heading_acts: Vec::new(),
            entries: Vec::new(),
        }]);
        let empty = page(&state);
        let _ = fs::remove_dir_all(&dir);
        assert!(!empty.contains("servers"), "{empty}");
    }

    #[test]
    fn listing_includes_readme() {
        let dir = tmp_dir("readme");
        fs::write(dir.join("README.md"), "# Hello listing\n\nA note.\n").unwrap();
        fs::write(dir.join("a.rs"), "fn main() {}\n").unwrap();
        let state = state_at(dir.clone());
        let root = state.cfg.root().expect("these tests always serve one");
        let html = listing_page(&state, &root, prefs(), &[], &VfsPath::root(), &[], "/");
        let _ = fs::remove_dir_all(&dir);
        assert!(html.contains("listing-readme"), "{html}");
        assert!(html.contains("Hello listing"), "{html}");
        assert!(html.contains("a.rs"), "{html}");
    }

    #[test]
    fn filtered_listing_skips_readme() {
        let dir = tmp_dir("readme-q");
        fs::write(dir.join("README.md"), "# Should not appear\n").unwrap();
        let state = state_at(dir.clone());
        let root = state.cfg.root().expect("these tests always serve one");
        let html = listing_page(
            &state,
            &root,
            prefs(),
            &[],
            &VfsPath::root(),
            &[("q".into(), "*.rs".into())],
            "/?q=*.rs",
        );
        let _ = fs::remove_dir_all(&dir);
        assert!(!html.contains("listing-readme"), "{html}");
        assert!(!html.contains("Should not appear"), "{html}");
    }
}
