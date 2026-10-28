//! LaTeX → MathML, server-side, and everything this crate does around
//! pulldown-latex to get there.
//!
//! The renderer is pulldown-latex 0.8. What it does not do, or does wrong, is
//! put right here and nowhere else, in two kinds:
//!
//! - **Before it parses** ([`prepare`]): a trailing `\tag{…}` is taken off and
//!   drawn by the page beside the formula ([`split_tag`]), and commands it has
//!   no entry for are spelled in ones it has ([`rewrite_commands`]).
//! - **After it renders** ([`finish`]): its output is mended where it is wrong.
//!
//! Each is a function of its own with its own tests, so that when an upstream
//! release does the thing itself the workaround is one function to delete.
//! The annotation keeps the source as written, `\tag` and all, so copying a
//! formula gives back what the document says.

use pulldown_latex::config::DisplayMode;
use pulldown_latex::{push_mathml, Parser as LatexParser, RenderConfig, Storage};

use crate::util::html_escape;

/// A formula as the page draws it: the MathML, or the red fallback for one
/// that does not parse, and its equation tag where it has one — `(4.1)`, ready
/// to show.
pub(crate) struct Rendered {
    pub html: String,
    pub tag: Option<String>,
}

/// Renders `latex` as display or inline MathML.
///
/// Broken math is not fatal: the source is shown in red with the parser
/// message as its tooltip, which keeps the rest of the document readable
/// (pulldown-latex's own `<merror>` output inlines the full multi-line
/// diagnostic, which swamps the page).
pub(crate) fn render(latex: &str, display: bool) -> Rendered {
    let (tex, tag) = prepare(latex);
    let storage = Storage::new();
    if let Some(err) = latex_error(&tex, &storage) {
        return Rendered { html: fallback(&err, latex), tag: None };
    }

    let parser = LatexParser::new(&tex, &storage);
    let config = RenderConfig {
        display_mode: if display {
            DisplayMode::Block
        } else {
            DisplayMode::Inline
        },
        // Keep the source in the output so copy/paste and assistive tech get
        // the original TeX — as written, not as rewritten for the parser.
        annotation: Some(latex),
        // Mid red: the renderer bakes this into the markup, so it has to be
        // legible against both the light and the dark background.
        error_color: (229, 83, 75),
        ..RenderConfig::default()
    };
    let mut out = String::new();
    match push_mathml(&mut out, parser, config) {
        Ok(()) => Rendered { html: finish(out), tag },
        Err(e) => Rendered { html: fallback(&e.to_string(), latex), tag: None },
    }
}

/// The source in red, with why as its tooltip.
fn fallback(why: &str, latex: &str) -> String {
    format!(
        "<code class=\"math-error\" title=\"{}\">{}</code>",
        html_escape(why),
        html_escape(latex.trim())
    )
}

/// First line of the first parse error, if the formula does not parse.
fn latex_error(latex: &str, storage: &Storage) -> Option<String> {
    LatexParser::new(latex, storage)
        .find_map(|ev| ev.err())
        .map(|e| {
            let msg = e.to_string();
            msg.lines().next().unwrap_or("invalid LaTeX").to_string()
        })
}

/// Everything done to the source before pulldown-latex sees it: the TeX it is
/// given, and the tag taken off it.
fn prepare(latex: &str) -> (String, Option<String>) {
    let (body, tag) = split_tag(latex);
    (rewrite_commands(body), tag)
}

/// **`\tag{X}`**, which pulldown-latex 0.8 does not know ("unknown primitive
/// command found"): taken off the end of the formula and returned as the label
/// to draw — `(X)`, or `X` as written for `\tag*{X}`. Only a tag that is the
/// last thing in the formula, braces balanced, which is where amsmath puts it
/// and the only place a document of ours does; anywhere else it is left for
/// the parser to refuse, as before.
fn split_tag(latex: &str) -> (&str, Option<String>) {
    let trimmed = latex.trim_end();
    let Some(at) = trimmed.rfind("\\tag") else {
        return (latex, None);
    };
    let rest = &trimmed[at + "\\tag".len()..];
    let (starred, rest) = match rest.strip_prefix('*') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let Some(inner) = rest.trim_start().strip_prefix('{') else {
        return (latex, None);
    };
    // The brace that closes the tag's own must be the formula's last
    // character: `\tag{a}{b}` or `\tag{a} x` is not a trailing tag.
    let mut depth = 1usize;
    for (i, c) in inner.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if i + 1 != inner.len() {
                        return (latex, None);
                    }
                    let label = inner[..i].trim();
                    let tag = match starred {
                        true => label.to_string(),
                        false => format!("({label})"),
                    };
                    return (&trimmed[..at], Some(tag));
                }
            }
            _ => {}
        }
    }
    (latex, None)
}

/// Commands pulldown-latex 0.8 has no entry for, in ones it has: amsmath's
/// limits with an arrow or a bar.
const REWRITES: &[(&str, &str)] = &[
    ("varprojlim", "\\underset{\\longleftarrow}{\\lim}"),
    ("varinjlim", "\\underset{\\longrightarrow}{\\lim}"),
    ("varliminf", "\\underline{\\lim}"),
    ("varlimsup", "\\overline{\\lim}"),
];

/// [`REWRITES`], applied to whole command names only: `\varprojlim` is
/// rewritten, `\varprojlimit` would not be.
fn rewrite_commands(tex: &str) -> String {
    let mut out = String::with_capacity(tex.len());
    let mut rest = tex;
    while let Some(at) = rest.find('\\') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let name_len = after.bytes().take_while(u8::is_ascii_alphabetic).count();
        let name = &after[..name_len];
        match REWRITES.iter().find(|(from, _)| *from == name) {
            Some((_, to)) if name_len > 0 => out.push_str(to),
            // Not ours: the backslash and the name (or the one character a
            // control symbol is, `\\` included) go through as written.
            _ => {
                let take = if name_len > 0 { name_len } else { after.chars().next().map_or(0, char::len_utf8) };
                out.push('\\');
                out.push_str(&after[..take]);
                rest = &after[take..];
                continue;
            }
        }
        rest = &after[name_len..];
    }
    out.push_str(rest);
    out
}

/// Everything done to pulldown-latex's output: `\ ` and `~` come out as the
/// text "&nbsp;" with its `&` escaped, so the page would show "&nbsp;"
/// literally; it is a no-break space.
fn finish(mathml: String) -> String {
    mathml.replace("&amp;nbsp;", "&#160;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_tag_is_taken_off_and_labelled() {
        assert_eq!(split_tag("a = b \\tag{4.1}"), ("a = b ", Some("(4.1)".into())));
        assert_eq!(split_tag("a = b \\tag*{A}\n"), ("a = b ", Some("A".into())));
        assert_eq!(split_tag("a \\tag {x_{1}}"), ("a ", Some("(x_{1})".into())));
        let block = "\\begin{aligned}a &= b\\\\ &\\le c\\end{aligned} \\tag{4.2}";
        assert_eq!(split_tag(block).1.as_deref(), Some("(4.2)"));
        assert!(split_tag(block).0.ends_with("\\end{aligned} "));
    }

    #[test]
    fn a_tag_not_at_the_end_is_left_alone() {
        for src in ["a \\tag{1} + b", "a \\tag{1}{2}", "a \\tag{1", "a + b", "\\tagged{x}"] {
            assert_eq!(split_tag(src), (src, None), "{src}");
        }
    }

    #[test]
    fn missing_limits_are_spelled_in_known_commands() {
        assert_eq!(
            rewrite_commands("\\varprojlim\\big(\\Gamma_n\\big)"),
            "\\underset{\\longleftarrow}{\\lim}\\big(\\Gamma_n\\big)"
        );
        assert_eq!(rewrite_commands("\\varlimsup_n x"), "\\overline{\\lim}_n x");
        // Whole names only, and everything else as it was, `\\` included.
        assert_eq!(rewrite_commands("\\varprojlimit a\\\\b \\, c"), "\\varprojlimit a\\\\b \\, c");
        assert_eq!(rewrite_commands("é \\alpha"), "é \\alpha");
    }

    #[test]
    fn tagged_and_rewritten_formulas_render() {
        for src in [
            "\\frac{a}{b} = 1. \\tag{3.1}",
            "\\begin{aligned} a &= b \\\\ &\\le d \\end{aligned} \\tag{4.2}",
            "\\varprojlim\\big(\\Gamma_n, \\sigma\\big)",
            "\\varinjlim A \\quad \\varliminf a_n \\quad \\varlimsup a_n",
        ] {
            let r = render(src, true);
            assert!(r.html.starts_with("<math"), "{src}: {}", r.html);
            assert!(!r.html.contains("math-error"), "{src}: {}", r.html);
            // The annotation is the source as written, in whichever escaping
            // the renderer gives it (it leaves `&` bare).
            assert!(r.html.contains(src) || r.html.contains(&html_escape(src)), "{src}: {}", r.html);
        }
        assert_eq!(render("x \\tag{2}", true).tag.as_deref(), Some("(2)"));
        // A formula that does not parse keeps its tag in the red source.
        let bad = render("\\frac{a \\tag{9}", true);
        assert!(bad.html.contains("math-error") && bad.tag.is_none(), "{}", bad.html);
    }
}
