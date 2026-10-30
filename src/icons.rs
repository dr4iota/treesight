//! The marks, in one place: every icon a page of ours draws, and every page
//! of an embedder's that wants the same picture, takes it from here.

// Icons are drawn, not typed. The obvious characters for these — folder,
// picture, film, note — all live in the emoji planes, and a font stack without
// them (any DejaVu-only Linux, for one) draws a missing-glyph box in their
// place. Paths cannot miss, they take the surrounding colour, and they stay
// legible in both themes. Each constant is the inside of an `<svg>`; `svg_icon`
// supplies the rest.
pub const ICON_FOLDER: &str = "<path d=\"M1.7 4.5c0-.5.4-.9.9-.9h2.9l1.3 1.7h7c.5 0 .9.4.9.9v6.4c0 \
     .5-.4.9-.9.9H2.6a.9.9 0 01-.9-.9V4.5z\"/>";
pub const ICON_IMAGE: &str = "<path d=\"M2 3.6h12v8.8H2z\"/><path d=\"M2.9 11.6l3.5-3.4 1.9 2 2.3-2.7 \
     3 3.4\"/><path d=\"M5.6 5.6a1.1 1.1 0 100 2.2 1.1 1.1 0 000-2.2z\"/>";
pub const ICON_AUDIO: &str = "<path d=\"M6.6 11.4V4l6.4-1.4v2.2L6.6 6.2\"/>\
     <path fill=\"currentColor\" stroke=\"none\" d=\"M4.9 9.6a2 2 0 100 4 2 2 0 000-4z\"/>";
pub const ICON_VIDEO: &str = "<path d=\"M1.9 3.9h12.2v8.2H1.9z\"/>\
     <path fill=\"currentColor\" stroke=\"none\" d=\"M6.6 6.3l4 1.7-4 1.7z\"/>";
pub const ICON_DOC: &str = "<path d=\"M3.6 2.4h5.6l3.2 3.2v8H3.6z\"/><path d=\"M9.2 2.4v3.2h3.2\"/>\
     <path d=\"M5.6 8.4h5M5.6 10.6h5\"/>";
pub const ICON_FILE: &str = "<path d=\"M3.6 2.4h5.6l3.2 3.2v8H3.6z\"/><path d=\"M9.2 2.4v3.2h3.2\"/>";
/// The file exactly as it is on disk: a plain sheet, the file rows' own mark,
/// with nothing drawn on it — no lines, as Preview has none to add.
pub const ICON_RAW: &str = "<path d=\"M3.6 2.4h5.6l3.2 3.2v8H3.6z\"/><path d=\"M9.2 2.4v3.2h3.2\"/>";
/// Download's mark turned upward: the bytes leaving for the tree.
pub const ICON_UPLOAD: &str =
    "<path d=\"M8 10.2V3.5\"/><path d=\"M5.2 6.1L8 3.3l2.8 2.8\"/><path d=\"M3.2 13.1h9.6\"/>";
/// The folder mark with a plus in it.
pub const ICON_NEW_FOLDER: &str = "<path d=\"M1.7 4.5c0-.5.4-.9.9-.9h2.9l1.3 1.7h7c.5 0 .9.4.9.9v6.4c0 \
     .5-.4.9-.9.9H2.6a.9.9 0 01-.9-.9V4.5z\"/><path d=\"M8 7.4v3.8M6.1 9.3h3.8\"/>";
pub const ICON_DOWNLOAD: &str =
    "<path d=\"M8 2.9v6.7\"/><path d=\"M5.2 7l2.8 2.8L10.8 7\"/><path d=\"M3.2 13.1h9.6\"/>";
/// A bin: Delete.
pub const ICON_DELETE: &str = "<path d=\"M2.8 4.4h10.4\"/><path d=\"M6.2 4.4V2.8h3.6v1.6\"/>\
     <path d=\"M4.2 4.4l.7 8.8h6.2l.7-8.8\"/>";
/// A filled bookmark: the badge on a row's icon whose toggle is on, or that is
/// in a state of the backend's it cannot change by itself.
pub const ICON_MARK: &str =
    "<path fill=\"currentColor\" d=\"M4.4 2.6h7.2v11l-3.6-2.6-3.6 2.6z\"/>";
/// The same bookmark outlined: the badge on a row's icon whose toggle is off.
pub const ICON_MARK_OPEN: &str = "<path d=\"M4.4 2.6h7.2v11l-3.6-2.6-3.6 2.6z\"/>";
/// The bookmark torn in two along a zigzag, the lower half fallen a little.
pub const ICON_MARK_TORN: &str = "<path fill=\"currentColor\" transform=\"rotate(-8 4.4 7)\" \
     d=\"M4.4 2.6h7.2v4.6l-1.2 1-1.2-1-1.2 1-1.2-1-1.2 1-1.2-1z\"/><path fill=\"currentColor\" \
     transform=\"translate(.9 1.4) rotate(6 11.6 9)\" d=\"M4.4 9l1.2-1 1.2 1 1.2-1 1.2 1 1.2-1 1.2 \
     1v4.6l-3.6-2.6-3.6 2.6z\"/>";
/// The bookmark whole, with a crack from its top edge in the background colour.
pub const ICON_MARK_CRACKED: &str = "<path fill=\"currentColor\" d=\"M4.4 2.6h7.2v11l-3.6-2.6-3.6 2.6z\"/>\
     <path class=\"crack\" d=\"M9.2 2.6l-1.3 1.6 1.2 1.1-1.4 1.5\"/>";

/// The filled bookmark in a look.
pub(crate) fn mark_in(look: crate::vfs::PinLook) -> &'static str {
    match look {
        crate::vfs::PinLook::Whole => ICON_MARK,
        crate::vfs::PinLook::Torn => ICON_MARK_TORN,
        crate::vfs::PinLook::Cracked => ICON_MARK_CRACKED,
    }
}
/// `</>`: the source, highlighted — the mark code is spelled with everywhere.
pub const ICON_SOURCE: &str = "<path d=\"M5.4 4.6L2.2 8l3.2 3.4\"/><path d=\"M10.6 4.6L13.8 8l-3.2 3.4\"/>\
     <path d=\"M9.1 3.2L6.9 12.8\"/>";
/// An eye: Preview, the file as this app shows it — the word and the mark an
/// editor uses for a rendered Markdown file.
pub const ICON_PREVIEW: &str = "<path d=\"M1.6 8s2.4-4.2 6.4-4.2S14.4 8 14.4 8s-2.4 4.2-6.4 4.2S1.6 8 1.6 8z\"/>\
     <path d=\"M8 6.1a1.9 1.9 0 100 3.8 1.9 1.9 0 000-3.8z\"/>";
/// Refresh: the page as the disk has it now. Three quarters of a circle with the
/// gap and the arrowhead at the top right, so the drawing is the turn itself. The
/// head is the chevron `ICON_UP` uses, rather than the square hook
/// most icon sets put on this one — an arrow is already spelled a particular way
/// here and a second spelling of it would only be a second spelling.
pub const ICON_REFRESH: &str =
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
pub const ICON_MENU: &str = "<path d=\"M3.6 5h8.8M3.6 8h8.8M3.6 11h8.8\"/>";

/// The way out of a directory: the `..` row, and the header's Up. Drawn like the
/// rest rather than typed as an arrow character, which has an advance width and
/// a baseline of its own and so never lined up with the icons beside it.
pub const ICON_UP: &str = "<path d=\"M8 12.8V3.8\"/><path d=\"M4.3 7.5L8 3.8l3.7 3.7\"/>";
/// The way out of the *root*, which is only a way out inside a shell: the start
/// page, reached by closing what is open.
///
/// Four panes, for the page that is four lists of what there is to open. It was
/// a house first, which is the mark every header spells "home" with — and the
/// wrong one here, because this is a file browser: a house beside a tree reads
/// as the *home folder*, which is a row in Places and not this at all. The word
/// went the same way, and for the same reason.
///
/// Not bars and not lines: the drawer button is three of those, and at the size
/// a header mark is drawn, how much of the box a set of strokes fills is the
/// whole of what tells two of them apart.
pub const ICON_START: &str = "<path d=\"M3.2 3.2h4v4h-4z\"/><path d=\"M8.8 3.2h4v4h-4z\"/>\
     <path d=\"M3.2 8.8h4v4h-4z\"/><path d=\"M8.8 8.8h4v4h-4z\"/>";
/// The theme flag says which setting is chosen rather than which one is next: a
/// sun for light, a moon for dark, and half of each for following the system.
/// Three settings, three drawings — resolving `auto` to the sun or the moon the
/// system happens to be on would draw it as an explicit choice, and lose the one
/// thing about that setting worth showing.
pub const ICON_SUN: &str = "<path d=\"M8 4.8a3.2 3.2 0 100 6.4 3.2 3.2 0 000-6.4z\"/>\
     <path d=\"M8 1.2v1.6M8 13.2v1.6M1.2 8h1.6M13.2 8h1.6M3.2 3.2l1.2 1.2\
     M11.6 11.6l1.2 1.2M12.8 3.2l-1.2 1.2M4.4 11.6l-1.2 1.2\"/>";
/// Centred on (8, 8) with a radius of 6, so it sits where the sun sits.
pub const ICON_MOON: &str = "<path d=\"M14 8.53A6 6 0 117.47 2 4.67 4.67 0 0014 8.53z\"/>";
/// Half filled, on the same circle. The fill runs out to the middle of the
/// stroke for the reason the pane's does: stopping at the inside of it leaves a
/// hairline seam. Unlike Line numbers and the pane, this fill is not a state — the flag has
/// three settings, so half-and-half means half of each, not "on".
pub const ICON_THEME_AUTO: &str = "<path d=\"M8 2a6 6 0 100 12A6 6 0 008 2z\"/>\
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
pub(crate) fn icon_pane(on: bool) -> String {
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
pub const ICON_CROSS: &str = "<path d=\"M4.5 4.5l7 7M11.5 4.5l-7 7\"/>";
// A star, outlined for a root that is not starred and filled for one that is.
// Two states of one shape rather than two shapes: the control stays in the same
// place and says which way round it is, which a second icon would not. Not the
// bookmark it was: that is a row's *kept offline* now, and one mark meaning two
// things in one pane was one too many.
pub const ICON_STAR: &str = "<path d=\"M8 2.3l1.75 3.55 3.9.55-2.83 2.75.67 3.88L8 11.2l-3.49 1.83.67-3.88L2.35 6.4l3.9-.55z\"/>";
pub const ICON_STARRED: &str =
    "<path fill=\"currentColor\" d=\"M8 2.3l1.75 3.55 3.9.55-2.83 2.75.67 3.88L8 11.2l-3.49 1.83.67-3.88L2.35 6.4l3.9-.55z\"/>";

/// "Serve this folder as the root": an arrow going in through the side of a frame.
/// Two windows, the front one filled: the main window, seen from another. The
/// same mark the embedder's terminal window draws for it.
pub const ICON_MAIN_WINDOW: &str = "<rect x=\"5.5\" y=\"2.5\" width=\"8\" height=\"6.5\" rx=\"1\"/>\
     <path d=\"M5.5 4.6h8\"/>\
     <rect x=\"2.5\" y=\"6.5\" width=\"8\" height=\"6.5\" rx=\"1\" fill=\"currentColor\" fill-opacity=\".25\"/>\
     <path d=\"M2.5 8.6h8\"/>";
/// A window with a plus at its corner: this, in a window of its own.
pub const ICON_NEW_WINDOW: &str = "<path d=\"M9 12.6H3.2a.8.8 0 01-.8-.8V4.2c0-.4.4-.8.8-.8h9.6c.4 0 .8.4.8.8V8\"/>\
     <path d=\"M2.4 5.8h11.2\"/><path d=\"M12 9.6v4M10 11.6h4\"/>";
pub const ICON_AS_ROOT: &str = "<path d=\"M9.8 3.4h2.8v9.2H9.8\"/><path d=\"M3.4 8h5.8\"/>\
     <path d=\"M6.8 5.6L9.2 8l-2.4 2.4\"/>";

/// "Close this folder": `ICON_AS_ROOT` the other way round, an arrow leaving
/// through the side of a frame. Not the cross, which on the Files heading — at
/// the top right of a drawer, on a phone — reads as closing the drawer.
pub const ICON_CLOSE_ROOT: &str = "<path d=\"M6.2 3.4H3.4v9.2h2.8\"/><path d=\"M6.8 8h5.8\"/>\
     <path d=\"M10.2 5.6L12.6 8l-2.4 2.4\"/>";

/// Every mark by name, for a page that draws them itself: an embedder hands
/// this table to its own pages rather than copying a path into them.
pub const ALL: &[(&str, &str)] = &[
    ("folder", ICON_FOLDER),
    ("image", ICON_IMAGE),
    ("audio", ICON_AUDIO),
    ("video", ICON_VIDEO),
    ("doc", ICON_DOC),
    ("file", ICON_FILE),
    ("raw", ICON_RAW),
    ("upload", ICON_UPLOAD),
    ("new-folder", ICON_NEW_FOLDER),
    ("download", ICON_DOWNLOAD),
    ("delete", ICON_DELETE),
    ("mark", ICON_MARK),
    ("mark-open", ICON_MARK_OPEN),
    ("mark-torn", ICON_MARK_TORN),
    ("mark-cracked", ICON_MARK_CRACKED),
    ("source", ICON_SOURCE),
    ("preview", ICON_PREVIEW),
    ("refresh", ICON_REFRESH),
    ("print", ICON_PRINT),
    ("menu", ICON_MENU),
    ("up", ICON_UP),
    ("start", ICON_START),
    ("sun", ICON_SUN),
    ("moon", ICON_MOON),
    ("theme-auto", ICON_THEME_AUTO),
    ("plus", ICON_PLUS),
    ("cross", ICON_CROSS),
    ("star", ICON_STAR),
    ("starred", ICON_STARRED),
    ("main-window", ICON_MAIN_WINDOW),
    ("new-window", ICON_NEW_WINDOW),
    ("as-root", ICON_AS_ROOT),
    ("close-root", ICON_CLOSE_ROOT),
];

/// One mark, drawn on a 16-unit grid.
///
/// The size here is a presentation attribute and therefore a *fallback*: it is
/// what a document gets when its stylesheet did not arrive. `--mark-size` in
/// `app.css` decides the size everywhere else, and is where to change it.
///
/// `class="mark"` is how the stylesheet finds them. It used to name the
/// containers instead — a dozen selectors, one per place a mark can sit — and
/// that list was wrong twice over within a day: the two buttons on the start
/// page were never in it, and neither was the Files heading. A mark's size is a
/// property of the mark, so it is asked of the mark.
pub fn svg_icon(paths: &str) -> String {
    format!(
        "<svg class=\"mark\" viewBox=\"0 0 16 16\" width=\"14\" height=\"14\" fill=\"none\" \
         stroke=\"currentColor\" stroke-width=\"1.2\" stroke-linecap=\"round\" \
         stroke-linejoin=\"round\" aria-hidden=\"true\">{paths}</svg>"
    )
}
