use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Write as _};

use comrak::adapters::SyntaxHighlighterAdapter;
use comrak::html::{dangerous_url, render_sourcepos, ChildRendering};
use comrak::nodes::{Node, NodeValue};
use comrak::options::Plugins;
use comrak::{create_formatter, parse_document, Arena, Options};
use pulldown_latex::config::DisplayMode;
use pulldown_latex::{push_mathml, Parser as LatexParser, RenderConfig, Storage};

use mermaid_rs_renderer::{render_with_options, RenderOptions, Theme};

use crate::hl::Hl;
use crate::util::html_escape;

/// A mermaid fence larger than this is shown as source rather than laid out.
const MAX_MERMAID_BYTES: usize = 64 * 1024;

/// Routes fenced code blocks through the same syntect pipeline used for
/// standalone files, so markdown code gets identical class-based highlighting.
struct CodeAdapter<'a> {
    hl: &'a Hl,
}

fn write_tag(
    out: &mut dyn fmt::Write,
    tag: &str,
    attrs: HashMap<&'static str, Cow<'_, str>>,
    extra_class: Option<&str>,
) -> fmt::Result {
    write!(out, "<{}", tag)?;
    let mut class_written = false;
    for (k, v) in &attrs {
        if *k == "class" {
            class_written = true;
            match extra_class {
                Some(c) => write!(out, " class=\"{} {}\"", c, html_escape(v))?,
                None => write!(out, " class=\"{}\"", html_escape(v))?,
            }
        } else {
            write!(out, " {}=\"{}\"", k, html_escape(v))?;
        }
    }
    if !class_written {
        if let Some(c) = extra_class {
            write!(out, " class=\"{}\"", c)?;
        }
    }
    write!(out, ">")
}

impl SyntaxHighlighterAdapter for CodeAdapter<'_> {
    fn write_highlighted(
        &self,
        output: &mut dyn fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> fmt::Result {
        let syntax = match lang {
            Some(l) if !l.is_empty() => self.hl.syntax_for_token(l),
            _ => self.hl.ss.find_syntax_plain_text(),
        };
        output.write_str(&self.hl.highlight(syntax, code))
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        write_tag(output, "pre", attributes, Some("hl-code"))
    }

    fn write_code_tag(
        &self,
        output: &mut dyn fmt::Write,
        attributes: HashMap<&'static str, Cow<'_, str>>,
    ) -> fmt::Result {
        write_tag(output, "code", attributes, None)
    }
}

/// LaTeX → MathML, server-side.
///
/// Broken math is not fatal: the source is shown in red with the parser
/// message as its tooltip, which keeps the rest of the document readable
/// (pulldown-latex's own `<merror>` output inlines the full multi-line
/// diagnostic, which swamps the page).
fn latex_to_mathml(latex: &str, display: bool) -> String {
    let storage = Storage::new();
    if let Some(err) = latex_error(latex, &storage) {
        return format!(
            "<code class=\"math-error\" title=\"{}\">{}</code>",
            html_escape(&err),
            html_escape(latex.trim())
        );
    }

    let parser = LatexParser::new(latex, &storage);
    let config = RenderConfig {
        display_mode: if display {
            DisplayMode::Block
        } else {
            DisplayMode::Inline
        },
        // Keep the source in the output so copy/paste and assistive tech get
        // the original TeX.
        annotation: Some(latex),
        // Mid red: the renderer bakes this into the markup, so it has to be
        // legible against both the light and the dark background.
        error_color: (229, 83, 75),
        ..RenderConfig::default()
    };
    let mut out = String::new();
    match push_mathml(&mut out, parser, config) {
        Ok(()) => out,
        Err(e) => format!(
            "<code class=\"math-error\" title=\"{}\">{}</code>",
            html_escape(&e.to_string()),
            html_escape(latex.trim())
        ),
    }
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

/// Replaces math nodes with pre-rendered MathML.
///
/// Comrak's math extensions only mark math up as `data-math-style` spans (they
/// assume a client-side typesetter), so we swap each one for a `Raw` node —
/// verbatim output, independent of the `unsafe` render option.
fn render_math_nodes<'a>(root: Node<'a>) {
    // Collect first: the tree is rewritten below.
    let mut targets: Vec<(Node<'a>, String, bool)> = Vec::new();
    for node in root.descendants() {
        match &node.data().value {
            NodeValue::Math(m) => targets.push((node, m.literal.clone(), m.display_math)),
            // ```math fences stay code blocks in the AST; comrak decides by
            // info string at render time.
            NodeValue::CodeBlock(cb) if info_lang(&cb.info) == "math" => {
                targets.push((node, cb.literal.trim_end_matches('\n').to_string(), true))
            }
            _ => {}
        }
    }

    for (node, latex, display) in targets {
        let mut target = node;
        // Display math standing alone gets block treatment. A paragraph holding
        // nothing but `$$...$$` is only a wrapper, so the formula replaces it
        // rather than being nested inside a <p>; display math mixed into a
        // paragraph's text has to stay inline to keep the HTML valid.
        let mut block = display;
        match node.parent() {
            Some(p) if matches!(p.data().value, NodeValue::Paragraph) => {
                if display && p.children().count() == 1 {
                    node.detach();
                    target = p;
                } else {
                    block = false;
                }
            }
            _ => {}
        }

        let mathml = latex_to_mathml(&latex, display);
        let html = if block {
            // The wrapper centers the formula and anchors equation numbers.
            format!("<div class=\"math-block\">{}</div>", mathml)
        } else {
            mathml
        };
        target.data_mut().value = NodeValue::Raw(html);
    }
}

/// First token of a code fence info string, as comrak splits it.
fn info_lang(info: &str) -> &str {
    info.split_whitespace().next().unwrap_or("")
}

/// Replaces ` ```mermaid ` fences with inline SVG (light and dark), the same
/// way math fences become MathML. Failure is not fatal: the source is shown
/// with the renderer message as a tooltip.
fn render_mermaid_nodes<'a>(root: Node<'a>) {
    let mut targets: Vec<(Node<'a>, String)> = Vec::new();
    for node in root.descendants() {
        if let NodeValue::CodeBlock(cb) = &node.data().value
            && info_lang(&cb.info).eq_ignore_ascii_case("mermaid")
        {
            targets.push((node, cb.literal.trim_end_matches('\n').to_string()));
        }
    }
    for (node, src) in targets {
        node.data_mut().value = NodeValue::Raw(render_mermaid_figure(&src));
    }
}

/// Light and dark SVG for a standalone mermaid source (a `.mmd` file, or the
/// body of a fence).
pub fn render_mermaid_figure(src: &str) -> String {
    match mermaid_svgs(src) {
        Ok((light, dark)) => format!(
            "<figure class=\"mermaid\"><div class=\"mermaid-light\">{}</div>\
             <div class=\"mermaid-dark\">{}</div></figure>",
            light, dark
        ),
        Err(err) => format!(
            "<pre class=\"mermaid-error\" title=\"{}\">{}</pre>",
            html_escape(&err),
            html_escape(src.trim())
        ),
    }
}

fn mermaid_svgs(src: &str) -> Result<(String, String), String> {
    if src.len() > MAX_MERMAID_BYTES {
        return Err("diagram too large to render".to_string());
    }
    if src.trim().is_empty() {
        return Err("empty diagram".to_string());
    }

    let light_opts = RenderOptions::modern();
    let dark_opts = RenderOptions {
        theme: Theme::dark(),
        ..RenderOptions::modern()
    };

    let light = render_with_options(src, light_opts).map_err(|e| mermaid_err_line(&e))?;
    let dark = render_with_options(src, dark_opts).map_err(|e| mermaid_err_line(&e))?;
    Ok((inline_svg(&light)?, inline_svg(&dark)?))
}

fn mermaid_err_line(err: &impl ToString) -> String {
    err.to_string()
        .lines()
        .next()
        .unwrap_or("invalid mermaid")
        .to_string()
}

/// Keep only SVG we can drop into HTML: no XML prologue, no script, no
/// event-handler attributes. The crate emits its own markup from an IR, so
/// this is defense in depth against a label that slipped through unescaped.
fn inline_svg(svg: &str) -> Result<String, String> {
    let t = svg.trim();
    if !t.starts_with("<svg") {
        return Err("renderer did not produce SVG".to_string());
    }
    let lower = t.to_ascii_lowercase();
    if lower.contains("<script") || lower.contains("javascript:") {
        return Err("renderer produced unsafe SVG".to_string());
    }
    if svg_has_on_handler(&lower) {
        return Err("renderer produced unsafe SVG".to_string());
    }
    Ok(t.to_string())
}

fn svg_has_on_handler(lower: &str) -> bool {
    let b = lower.as_bytes();
    let mut in_tag = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'<' => in_tag = true,
            b'>' => in_tag = false,
            b' ' | b'\t' | b'\n' | b'\r' | b'/'
                if in_tag
                    && b.get(i + 1) == Some(&b'o')
                    && b.get(i + 2) == Some(&b'n')
                    && b.get(i + 3).is_some_and(u8::is_ascii_alphabetic) =>
            {
                let mut j = i + 3;
                while j < b.len() && b[j].is_ascii_alphabetic() {
                    j += 1;
                }
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < b.len() && b[j] == b'=' {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Rewrites the LaTeX-native delimiters — `\(x\)` and `\[x\]`, KaTeX's own
/// defaults — into the dollar forms comrak understands.
///
/// This has to happen before parsing: to CommonMark, `\(` is just an escaped
/// parenthesis, so the backslashes are gone by the time there is an AST.
/// Content inside code spans and fenced code blocks is left untouched, and a
/// delimiter is only rewritten when its partner is found, so stray `\[`
/// escapes in prose keep their current meaning. Indentation-only code blocks
/// are not tracked: a `\(` in one is rewritten to `$` and shows as such. That
/// is the one case this trades away, because telling four-space code from an
/// indented paragraph inside a list needs the block structure we don't have yet.
fn expand_tex_delimiters(src: &str) -> Cow<'_, str> {
    if !src.contains("\\(") && !src.contains("\\[") {
        return Cow::Borrowed(src);
    }

    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(src.len() + 32);
    let mut i = 0;
    let mut at_line_start = true;
    // Open fence, as (fence char, length).
    let mut fence: Option<(u8, usize)> = None;

    while i < b.len() {
        // Fenced code blocks are line-oriented; copy them through verbatim,
        // opening and closing fence lines included.
        if at_line_start {
            let marker = fence_marker(b, i);
            let in_code = match (fence, marker) {
                (Some((open_c, open_len)), Some((c, len, after))) => {
                    if c == open_c && len >= open_len && rest_of_line_blank(b, after) {
                        fence = None;
                    }
                    true
                }
                (Some(_), None) => true,
                (None, Some((c, len, _))) => {
                    fence = Some((c, len));
                    true
                }
                (None, None) => false,
            };
            if in_code {
                let end = line_end(b, i);
                out.extend_from_slice(&b[i..end]);
                i = end;
                continue; // still at a line start
            }
        }

        at_line_start = false;
        match b[i] {
            b'\n' => {
                out.push(b'\n');
                i += 1;
                at_line_start = true;
            }
            // Code span: copy through the matching backtick run.
            b'`' => {
                let n = run_len(b, i, b'`');
                let end = match find_run(b, i + n, b'`', n) {
                    Some(close) => close + n,
                    None => i + n,
                };
                out.extend_from_slice(&b[i..end]);
                i = end;
            }
            // Existing display math: copy through the closing `$$`.
            b'$' if b.get(i + 1) == Some(&b'$') => {
                let end = match find_bytes(b, i + 2, b"$$") {
                    Some(close) => close + 2,
                    None => i + 2,
                };
                out.extend_from_slice(&b[i..end]);
                i = end;
            }
            b'\\' => match b.get(i + 1) {
                // `\(x\)` is inline math and stays within one paragraph.
                Some(b'(') => match tex_span(src, i + 2, "\\)") {
                    Some((inner, end)) => {
                        out.push(b'$');
                        out.extend_from_slice(inner.trim().as_bytes());
                        out.push(b'$');
                        i = end;
                    }
                    None => {
                        out.extend_from_slice(&b[i..i + 2]);
                        i += 2;
                    }
                },
                // `\[x\]` is display math and may span lines.
                Some(b'[') => match tex_span(src, i + 2, "\\]") {
                    Some((inner, end)) => {
                        out.extend_from_slice(b"$$");
                        out.extend_from_slice(inner.as_bytes());
                        out.extend_from_slice(b"$$");
                        i = end;
                    }
                    None => {
                        out.extend_from_slice(&b[i..i + 2]);
                        i += 2;
                    }
                },
                // Any other escape, `\\` included, passes through as a unit so
                // its second byte is never mistaken for a delimiter.
                Some(_) => {
                    out.extend_from_slice(&b[i..i + 2]);
                    i += 2;
                }
                None => {
                    out.push(b'\\');
                    i += 1;
                }
            },
            _ => {
                out.push(b[i]);
                i += 1;
            }
        }
    }

    // Only whole characters and ASCII delimiters were copied, so this holds.
    match String::from_utf8(out) {
        Ok(s) => Cow::Owned(s),
        Err(_) => Cow::Borrowed(src),
    }
}

/// Content of a math span starting at `from`, plus the index just past its
/// closing delimiter.
///
/// The search stops at a blank line: display math may run over several lines,
/// but neither form crosses a paragraph, and without that limit an opener with
/// no partner would swallow text up to some unrelated closer later in the file.
/// A span containing `$` is rejected too, since rewriting it would produce
/// delimiters comrak would then mis-pair.
fn tex_span<'a>(src: &'a str, from: usize, close: &str) -> Option<(&'a str, usize)> {
    let b = src.as_bytes();
    let mut i = from;
    while i < b.len() {
        if src[i..].starts_with(close) {
            let inner = &src[from..i];
            if inner.trim().is_empty() || inner.contains('$') {
                return None;
            }
            return Some((inner, i + close.len()));
        }
        if src[i..].starts_with("\n\n") {
            return None;
        }
        // Skip escapes so `\\` before the closing bracket doesn't confuse us.
        i += if b[i] == b'\\' && i + 1 < b.len() { 2 } else { 1 };
    }
    None
}

/// Length of the run of `c` starting at `i`.
fn run_len(b: &[u8], i: usize, c: u8) -> usize {
    b[i..].iter().take_while(|&&x| x == c).count()
}

/// Start of the next run of exactly `n` occurrences of `c`, at or after `from`.
fn find_run(b: &[u8], from: usize, c: u8, n: usize) -> Option<usize> {
    let mut i = from;
    while i < b.len() {
        if b[i] == c {
            let len = run_len(b, i, c);
            if len == n {
                return Some(i);
            }
            i += len;
        } else {
            i += 1;
        }
    }
    None
}

fn find_bytes(b: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    (from..b.len().saturating_sub(needle.len() - 1)).find(|&i| &b[i..i + needle.len()] == needle)
}

fn line_end(b: &[u8], from: usize) -> usize {
    match b[from..].iter().position(|&c| c == b'\n') {
        Some(p) => from + p + 1,
        None => b.len(),
    }
}

/// Code fence marker on the line beginning at `i`, as (char, length, index
/// just past the run). `None` when the line neither opens nor closes a fence.
/// Up to three leading spaces are allowed, as in CommonMark.
fn fence_marker(b: &[u8], i: usize) -> Option<(u8, usize, usize)> {
    let mut p = i;
    while p < b.len() && p - i < 3 && b[p] == b' ' {
        p += 1;
    }
    if p < b.len() && (b[p] == b'`' || b[p] == b'~') {
        let len = run_len(b, p, b[p]);
        if len >= 3 {
            return Some((b[p], len, p + len));
        }
    }
    None
}

fn rest_of_line_blank(b: &[u8], from: usize) -> bool {
    b[from..]
        .iter()
        .take_while(|&&c| c != b'\n')
        .all(|&c| c == b' ' || c == b'\t')
}

fn md_options() -> Options<'static> {
    let mut options = Options::default();
    // GitHub Flavored Markdown.
    options.extension.strikethrough = true;
    options.extension.tagfilter = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.render.gfm_quirks = true;
    options.render.tasklist_classes = true;
    // GitHub extras beyond the GFM spec.
    options.extension.footnotes = true;
    options.extension.alerts = true;
    options.extension.header_id_prefix = Some(String::new());
    // Math: `$x$` / `$$x$$` and `$`x`$` / ```math fences.
    options.extension.math_dollars = true;
    options.extension.math_code = true;
    // Local files viewed by their owner: allow embedded raw HTML, minus the
    // handful of tags GFM's tagfilter neutralizes (script, iframe, style, …).
    options.render.r#unsafe = true;
    options
}

pub fn render_markdown(hl: &Hl, src: &str) -> String {
    let options = md_options();

    let adapter = CodeAdapter { hl };
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);

    let src = expand_tex_delimiters(src);

    let arena = Arena::new();
    let root = parse_document(&arena, &src, &options);
    render_math_nodes(root);
    render_mermaid_nodes(root);

    let mut out = String::with_capacity(src.len() * 3 / 2);
    match Links::format_document_with_plugins(root, &options, &mut out, &plugins) {
        Ok(()) => out,
        Err(_) => format!("<pre>{}</pre>", html_escape(&src)),
    }
}

/// A link that leaves the tree: anything carrying a scheme, and anything
/// carrying a host.
///
/// A markdown file has no business knowing what host it is being served from,
/// so the two shapes that name one are the two that mean "somewhere else":
/// `https://example.com/x`, and `//example.com/x` borrowing the page's own
/// scheme. Everything else — `./note.md`, `/src/main.rs`, `?raw=1`, `#heading`
/// — is a place in this tree, and opens in place.
///
/// The grammar is the URL one: a scheme starts with a letter, continues in
/// letters, digits, `+`, `-` and `.`, and ends at the first `:` — which has to
/// come before any `/`, `?` or `#`, since each of those means the rest is a
/// path. `notes:2024.md` is therefore a scheme and not a file, which is how a
/// browser reads it too; a file named that way is linked as `./notes:2024.md`.
fn leaves_the_tree(url: &str) -> bool {
    if let Some(host) = url.strip_prefix("//") {
        return !host.is_empty();
    }
    let Some(colon) = url
        .find([':', '/', '?', '#'])
        .filter(|i| url.as_bytes()[*i] == b':')
    else {
        return false;
    };
    let mut scheme = url[..colon].chars();
    scheme.next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

create_formatter!(Links, {
    // comrak's own, plus the two attributes: a link out of the tree opens in a
    // tab of its own in a browser, and the shell turns that request into the
    // system browser rather than a second window. `noopener noreferrer` because
    // a page opened this way has no business holding a handle on this one, and
    // a local path is nobody's referrer.
    //
    // Everything before the attributes is what `render_link` writes, kept
    // byte-for-byte so that a link that stays in the tree renders as it always
    // has: the tests below pin that, and so do the snapshots.
    //
    // Only markdown's own links, written or autolinked. An `<a>` written as raw
    // HTML in the document passes through as the author typed it, because it
    // arrives here as text and not as a link — in the shell the navigation
    // handler still sends it to the browser by origin, and in a plain browser
    // it opens where the author said it would.
    NodeValue::Link(ref nl) => |context, node, entering| {
        if !entering {
            context.write_str("</a>")?;
            return Ok(ChildRendering::HTML);
        }
        context.write_str("<a")?;
        render_sourcepos(context, node)?;
        context.write_str(" href=\"")?;
        if context.options.render.r#unsafe || !dangerous_url(&nl.url) {
            context.escape_href(&nl.url)?;
        }
        context.write_str("\"")?;
        if !nl.title.is_empty() {
            context.write_str(" title=\"")?;
            context.escape(&nl.title)?;
            context.write_str("\"")?;
        }
        if leaves_the_tree(&nl.url) {
            context.write_str(" target=\"_blank\" rel=\"noopener noreferrer\"")?;
        }
        context.write_str(">")?;
    },
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hl::Hl;

    fn html(src: &str) -> String {
        render_markdown(&Hl::for_tests(), src)
    }

    #[test]
    fn mermaid_fence_becomes_dual_svg() {
        let out = html("```mermaid\nflowchart LR\n    A --> B\n```\n");
        assert!(out.contains("class=\"mermaid\""), "{out}");
        assert!(out.contains("mermaid-light"), "{out}");
        assert!(out.contains("mermaid-dark"), "{out}");
        assert_eq!(out.matches("<svg").count(), 2, "{out}");
    }

    #[test]
    fn mermaid_error_keeps_source() {
        let out = html("```mermaid\nthis is not a diagram\n```\n");
        assert!(out.contains("mermaid-error"), "{out}");
        assert!(out.contains("this is not a diagram"), "{out}");
        assert!(!out.contains("<svg"), "{out}");
    }

    #[test]
    fn rust_fence_is_not_mermaid() {
        let out = html("```rust\nfn main() {}\n```\n");
        assert!(out.contains("hl-code"), "{out}");
        assert!(!out.contains("class=\"mermaid\""), "{out}");
    }

    #[test]
    fn inline_mermaid_token_stays_code() {
        let out = html("see `mermaid` in a sentence\n");
        assert!(out.contains("<code>mermaid</code>"), "{out}");
        assert!(!out.contains("class=\"mermaid\""), "{out}");
    }

    #[test]
    fn math_dollar_and_fence() {
        let inline = html("a $x$ b\n");
        assert!(inline.contains("<math"), "{inline}");
        let block = html("```math\nx^2\n```\n");
        assert!(block.contains("<math"), "{block}");
        assert!(block.contains("math-block"), "{block}");
    }

    #[test]
    fn tex_delimiters_in_prose() {
        assert_eq!(expand_tex_delimiters(r"a \(x\) b").as_ref(), "a $x$ b");
        assert_eq!(expand_tex_delimiters("a \\[x\\] b").as_ref(), "a $$x$$ b");
    }

    #[test]
    fn tex_delimiters_leave_fences_alone() {
        let src = "```\n\\(x\\)\n```\n";
        assert_eq!(expand_tex_delimiters(src).as_ref(), src);
    }

    /// A scheme or a host means somewhere that is not this tree, and a markdown
    /// file is not supposed to know which host it is being served from.
    #[test]
    fn a_url_that_names_a_host_leaves_the_tree() {
        for out in [
            "https://example.com/x",
            "http://example.com",
            "//example.com/x",
            "mailto:a@b.c",
            "ftp://example.com/f",
            "notes:2024.md",
        ] {
            assert!(leaves_the_tree(out), "{out} names somewhere else");
        }
        for here in [
            "./note.md",
            "note.md",
            "/src/main.rs",
            "sub/c.md",
            "?raw=1",
            "#heading",
            "",
            "//",
            "1st:not-a-scheme.md",
        ] {
            assert!(!leaves_the_tree(here), "{here} is a place in this tree");
        }
    }

    #[test]
    fn an_external_link_opens_in_a_tab_of_its_own() {
        let out = html("[docs](https://example.com/x) and <https://example.com/bare>\n");
        assert_eq!(
            out.matches("target=\"_blank\" rel=\"noopener noreferrer\"").count(),
            2,
            "the written link and the autolink both leave: {out}"
        );
    }

    /// The other half, and the one a snapshot would catch: a link that stays
    /// renders exactly as it always did, attributes and all.
    #[test]
    fn a_link_that_stays_is_untouched() {
        let out = html("[here](./note.md) [titled](/a.rs \"a title\")\n");
        assert!(out.contains("<a href=\"./note.md\">here</a>"), "{out}");
        assert!(
            out.contains("<a href=\"/a.rs\" title=\"a title\">titled</a>"),
            "{out}"
        );
        assert!(!out.contains("_blank"), "{out}");
    }
}
