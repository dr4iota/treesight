use syntect::html::{css_for_theme_with_class_style, ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName};

/// All generated span classes are prefixed so they cannot collide with page CSS.
/// The generated theme CSS targets the container class ".hl-code".
pub const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };

pub struct Hl {
    pub ss: SyntaxSet,
    pub css_light: String,
    pub css_dark: String,
}

/// All themes are embedded in the binary by two-face and lazily
/// deserialized, so any of them can be selected at run time.
pub fn theme_names() -> impl Iterator<Item = &'static str> {
    EmbeddedLazyThemeSet::theme_names()
        .iter()
        .map(|t| t.as_name())
}

/// Case/punctuation-insensitive lookup: "one-half-dark" == "OneHalfDark".
pub fn find_theme(input: &str) -> Option<EmbeddedThemeName> {
    let norm = |s: &str| -> String {
        s.chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let want = norm(input);
    EmbeddedLazyThemeSet::theme_names()
        .iter()
        .copied()
        .find(|t| norm(t.as_name()) == want)
}

impl Hl {
    pub fn new(light: EmbeddedThemeName, dark: EmbeddedThemeName) -> Hl {
        let ss = two_face::syntax::extra_newlines();
        let themes = two_face::theme::extra();
        let css_light = css_for_theme_with_class_style(themes.get(light), CLASS_STYLE)
            .expect("light theme css");
        let css_dark =
            css_for_theme_with_class_style(themes.get(dark), CLASS_STYLE).expect("dark theme css");
        Hl { ss, css_light, css_dark }
    }

    #[cfg(test)]
    pub fn for_tests() -> Hl {
        use std::sync::OnceLock;
        static HL: OnceLock<Hl> = OnceLock::new();
        let hl = HL.get_or_init(|| {
            Hl::new(
                EmbeddedThemeName::InspiredGithub,
                EmbeddedThemeName::OneHalfDark,
            )
        });
        Hl {
            ss: hl.ss.clone(),
            css_light: hl.css_light.clone(),
            css_dark: hl.css_dark.clone(),
        }
    }

    /// Pick a syntax for a file: extension, then full name (Makefile etc.),
    /// then shebang / first line, then plain text.
    ///
    /// **Not by a number.** `syslog.2` and `messages.1` — a rotated log, and
    /// what a `.gz` of one unpacks as — are far likelier than a man page's
    /// source, which syntect claims every digit for and which says so in its
    /// first line anyway. A numbered name goes by its first line or is text.
    pub fn syntax_for(&self, name: &str, ext: &str, first_line: &str) -> &SyntaxReference {
        let numbered = !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_digit());
        (!numbered)
            .then(|| self.ss.find_syntax_by_extension(ext))
            .flatten()
            .or_else(|| self.ss.find_syntax_by_extension(name))
            .or_else(|| self.ss.find_syntax_by_first_line(first_line))
            .unwrap_or_else(|| self.ss.find_syntax_plain_text())
    }

    pub fn syntax_for_token(&self, token: &str) -> &SyntaxReference {
        self.ss
            .find_syntax_by_token(token)
            .unwrap_or_else(|| self.ss.find_syntax_plain_text())
    }

    /// Highlight text into class-annotated HTML spans (no surrounding <pre>).
    pub fn highlight(&self, syntax: &SyntaxReference, text: &str) -> String {
        let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, &self.ss, CLASS_STYLE);
        for line in LinesWithEndings::from(text) {
            if generator.parse_html_for_line_which_includes_newline(line).is_err() {
                // Highlighting failed mid-way; fall back to escaped plain text.
                return crate::util::html_escape(text);
            }
        }
        generator.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::Hl;

    #[test]
    fn a_numbered_name_is_a_rotated_log_not_a_man_page() {
        let hl = Hl::for_tests();
        let log = "Oct 12 10:00:00 thor sshd[1000]: Accepted password";
        assert_eq!(hl.syntax_for("syslog.2", "2", log).name, "Plain Text");
        assert_eq!(hl.syntax_for("messages.1", "1", log).name, "Plain Text");
        // A script that happens to be numbered still goes by its first line.
        assert_eq!(hl.syntax_for("run.1", "1", "#!/bin/bash").name, "Bourne Again Shell (bash)");
        // And an ordinary extension is untouched.
        assert_eq!(hl.syntax_for("a.rs", "rs", "").name, "Rust");
    }
}
