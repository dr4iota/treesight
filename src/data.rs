//! Data files drawn as what they hold rather than as lines of text: a CSV or
//! TSV as a table, and JSON laid out one member to a line — and a gzipped
//! file opened first, so a rotated log reads as the log. Pure functions of
//! the text; `view.rs` decides when they run and keeps the highlighted source
//! a flag away.

use std::fmt::Write as _;
use std::io::Read;

use flate2::read::MultiGzDecoder;

use crate::util::html_escape;

/// Rows past this are left out of a table, and the meta line says how many.
/// A browser lays out tens of thousands of rows slowly and a phone worse, and
/// nobody reads that many in a pane; Raw and Download still have them all.
pub const MAX_TABLE_ROWS: usize = 5000;

/// A cell longer than this wraps; one this long or shorter keeps to its line.
const LONG_CELL: usize = 40;

/// The separator a delimited file is split on: a tab for TSV, and for CSV a
/// comma unless the first line has more semicolons than commas — which is what
/// a spreadsheet writes where the comma is the decimal point.
pub fn delimiter_for(ext: &str, text: &str) -> char {
    if ext != "csv" {
        return '\t';
    }
    let first = text.lines().next().unwrap_or("");
    let count = |c: char| first.chars().filter(|&x| x == c).count();
    match count(';') > count(',') {
        true => ';',
        false => ',',
    }
}

/// RFC 4180, leniently: quoted fields with doubled quotes and line breaks
/// inside, CRLF or LF, a quote in the middle of an unquoted field kept as a
/// character, and rows of any length. A blank line is not a row, and neither is
/// the end of the file after a final line break. A leading BOM is dropped.
pub fn parse_delimited(text: &str, delim: char) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    // A field that opened with a quote: the empty `""` is a field, where an
    // empty line is not a row.
    let mut quoted = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => in_quotes = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !quoted => {
                in_quotes = true;
                quoted = true;
            }
            c if c == delim => {
                row.push(std::mem::take(&mut field));
                quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                if quoted || !field.is_empty() || !row.is_empty() {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                quoted = false;
            }
            _ => field.push(c),
        }
    }
    if quoted || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// A cell that is a number, so its column of figures lines up on the right:
/// a sign, digits grouped by commas or underscores, one decimal point, an
/// exponent, a closing `%`. A date, a phone number or a version is not one —
/// its hyphens and second dot are not where a number has them.
fn is_number(s: &str) -> bool {
    let s = s.trim();
    let s = s.strip_prefix(['-', '+']).unwrap_or(s);
    let s = s.strip_suffix('%').unwrap_or(s);
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mut digits = false;
    let mut dot = false;
    let mut prev_digit = false;
    for c in mantissa.chars() {
        match c {
            '0'..='9' => digits = true,
            '.' if !dot => dot = true,
            ',' | '_' if prev_digit && !dot => {}
            _ => return false,
        }
        prev_digit = c.is_ascii_digit();
    }
    if !digits || mantissa.ends_with([',', '_']) {
        return false;
    }
    match exponent {
        None => true,
        Some(e) => {
            let e = e.strip_prefix(['-', '+']).unwrap_or(e);
            !e.is_empty() && e.chars().all(|c| c.is_ascii_digit())
        }
    }
}

/// Columns past this and the file is not drawn as a table. Nobody reads a
/// thousand columns across a screen, and it bounds a row's work before the
/// size check below sees it: one line of two million commas is two million
/// cells in every row padded to it.
pub const MAX_TABLE_COLS: usize = 1000;

/// The first row as the heading, the rest as the body, every cell escaped and
/// every row padded to the widest one shown. `None` for a file with no rows,
/// which the source view says better; `Some(Err(()))` for one wider than
/// `MAX_TABLE_COLS` or whose table would pass `cap` bytes, checked row by row.
pub fn render_table(rows: &[Vec<String>], cap: usize) -> Option<Result<String, ()>> {
    let (head, body) = rows.split_first()?;
    let body = &body[..body.len().min(MAX_TABLE_ROWS)];
    let width = body.iter().chain([head]).map(Vec::len).max().unwrap_or(0);
    if width > MAX_TABLE_COLS {
        return Some(Err(()));
    }
    let mut out = String::from("<div class=\"tablewrap\"><table class=\"data\"><thead><tr>");
    for i in 0..width {
        let _ = write!(out, "<th>{}</th>", html_escape(head.get(i).map(String::as_str).unwrap_or("")));
    }
    out.push_str("</tr></thead><tbody>");
    for row in body {
        out.push_str("<tr>");
        for i in 0..width {
            let cell = row.get(i).map(String::as_str).unwrap_or("");
            // A table squeezed into a phone wraps at every break it can find,
            // a date at its hyphens. Short cells keep to one line, then, and a
            // long one wraps at a width that still reads as a paragraph.
            let class = match (is_number(cell), cell.chars().count() > LONG_CELL) {
                (true, _) => " class=\"n\"",
                (false, true) => " class=\"w\"",
                (false, false) => "",
            };
            let _ = write!(out, "<td{class}>{}</td>", html_escape(cell));
        }
        out.push_str("</tr>");
        if out.len() > cap {
            return Some(Err(()));
        }
    }
    out.push_str("</tbody></table></div>");
    Some(Ok(out))
}

/// JSON laid out two spaces to a level, one member or element to a line, with
/// every token copied as written — a number keeps its digits and a string its
/// escapes, which a parse into values and back would not promise. `None` for
/// anything that is not JSON as far as the layout can tell: unbalanced
/// brackets, an unterminated string, a comment (a `tsconfig.json` has them),
/// or nothing at all. The source view then draws the file as it is.
///
/// Several values in a row are laid out one after another, which is also what
/// JSON Lines is when every record is on a line of its own.
///
/// `None` too past `cap` bytes of output or `MAX_JSON_DEPTH` levels, checked
/// as it goes: indentation grows with depth, so a 40 KB file of nested
/// brackets would lay out to most of a gigabyte before a check at the end.
/// The page then draws the file as written.
pub fn reformat_json(text: &str, cap: usize) -> Option<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = String::with_capacity((text.len() * 2).min(cap));
    let mut stack: Vec<char> = Vec::new();
    // What may come next, which is what makes this a check and not only a
    // layout: `[1 2]` is not two numbers run together, and `hello world` is
    // not JSON however it is laid out.
    let mut want = Want::Value;
    // Values at the top level, one after another.
    let mut values = 0usize;
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if is_json_space(c) {
            continue;
        }
        let starts_value = matches!(c, '{' | '[' | '"') || is_literal_char(c);
        if stack.is_empty() && starts_value {
            // A value at the top: the first, or the next of several in a row.
            if !matches!(want, Want::Value | Want::After) {
                return None;
            }
            if values > 0 {
                out.push('\n');
            }
            values += 1;
            want = Want::Value;
        }
        match c {
            '"' => {
                want = match want {
                    Want::Value => Want::After,
                    Want::Key => Want::Colon,
                    _ => return None,
                };
                // As JSON spells a string: no raw control character, and only
                // its own escapes, `\u` with all four digits.
                out.push('"');
                loop {
                    let c = chars.next()?;
                    if c < ' ' {
                        return None;
                    }
                    out.push(c);
                    match c {
                        '\\' => {
                            let e = chars.next()?;
                            out.push(e);
                            match e {
                                '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {}
                                'u' => {
                                    for _ in 0..4 {
                                        let h = chars.next().filter(char::is_ascii_hexdigit)?;
                                        out.push(h);
                                    }
                                }
                                _ => return None,
                            }
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '{' | '[' => {
                if want != Want::Value {
                    return None;
                }
                let close = if c == '{' { '}' } else { ']' };
                while chars.next_if(|&c| is_json_space(c)).is_some() {}
                if chars.next_if_eq(&close).is_some() {
                    out.push(c);
                    out.push(close);
                    want = Want::After;
                    continue;
                }
                out.push(c);
                stack.push(close);
                if stack.len() > MAX_JSON_DEPTH {
                    return None;
                }
                want = if c == '{' { Want::Key } else { Want::Value };
                newline(&mut out, stack.len());
            }
            '}' | ']' => {
                if want != Want::After || stack.pop()? != c {
                    return None;
                }
                newline(&mut out, stack.len());
                out.push(c);
            }
            ',' => {
                if want != Want::After || stack.is_empty() {
                    return None;
                }
                // A comma before a closing bracket is the other thing that
                // makes a `tsconfig.json` not JSON, and it goes the way a
                // comment does: drawn as written, not laid out around a gap.
                while chars.next_if(|&c| is_json_space(c)).is_some() {}
                if matches!(chars.peek(), Some('}' | ']')) {
                    return None;
                }
                out.push(',');
                want = if stack.last() == Some(&'}') { Want::Key } else { Want::Value };
                newline(&mut out, stack.len());
            }
            ':' => {
                if want != Want::Colon {
                    return None;
                }
                out.push_str(": ");
                want = Want::Value;
            }
            c if is_literal_char(c) => {
                if want != Want::Value {
                    return None;
                }
                // The whole word, then whether it is one JSON has.
                let start = out.len();
                out.push(c);
                while let Some(c) = chars.next_if(|&c| is_literal_char(c)) {
                    out.push(c);
                }
                if !is_json_literal(&out[start..]) {
                    return None;
                }
                want = Want::After;
            }
            _ => return None,
        }
        if out.len() > cap {
            return None;
        }
    }
    if !stack.is_empty() || values == 0 || want != Want::After {
        return None;
    }
    out.push('\n');
    Some(out)
}

/// What `reformat_json` may take next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Want {
    /// A value: at the start, after `[`, `:` or an array's `,`.
    Value,
    /// A member's name: after `{` or an object's `,`.
    Key,
    /// The `:` after a name.
    Colon,
    /// After a whole value: `,` or the bracket that closes it, or at the top
    /// level the next value or the end.
    After,
}

/// `true`, `false`, `null`, or a number as JSON spells one: no leading `+`,
/// no leading zero, digits both sides of a point, an exponent with digits.
fn is_json_literal(s: &str) -> bool {
    if matches!(s, "true" | "false" | "null") {
        return true;
    }
    let b = s.strip_prefix('-').unwrap_or(s).as_bytes();
    let digits = |b: &[u8]| b.iter().take_while(|c| c.is_ascii_digit()).count();
    let int = digits(b);
    if int == 0 || (int > 1 && b[0] == b'0') {
        return false;
    }
    let mut rest = &b[int..];
    if let Some(r) = rest.strip_prefix(b".") {
        let n = digits(r);
        if n == 0 {
            return false;
        }
        rest = &r[n..];
    }
    if let Some(r) = rest.strip_prefix(b"e").or_else(|| rest.strip_prefix(b"E")) {
        let r = r.strip_prefix(b"+").or_else(|| r.strip_prefix(b"-")).unwrap_or(r);
        let n = digits(r);
        if n == 0 {
            return false;
        }
        rest = &r[n..];
    }
    rest.is_empty()
}

/// How deep `reformat_json` lays out. `serde_json`'s own default recursion
/// limit, so what is past it is past most JSON tooling too; and a line there
/// is already 256 columns of indentation before its first character.
pub const MAX_JSON_DEPTH: usize = 128;

/// JSON's whitespace, which is these four and no other: a no-break space
/// between two values is not JSON.
fn is_json_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// What a number, `true`, `false` or `null` is spelled with.
fn is_literal_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.')
}

/// What a `.gz` holds, up to `cap` bytes, and whether there was more. Read
/// through a `take`, so a small file that inflates to gigabytes stops at the
/// cap rather than filling memory. `None` for something that is not gzip after
/// all. Members in a row are read as one stream, as `zcat` reads them.
pub fn gunzip(read: impl Read, cap: u64) -> Option<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    MultiGzDecoder::new(read).take(cap + 1).read_to_end(&mut out).ok()?;
    let more = out.len() as u64 > cap;
    out.truncate(cap as usize);
    Some((out, more))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: usize = 1 << 20;

    fn rows(text: &str) -> Vec<Vec<String>> {
        parse_delimited(text, ',')
    }

    #[test]
    fn quotes_hold_separators_line_breaks_and_quotes() {
        let got = rows("a,b\r\n\"x, y\",\"two\nlines\"\r\n\"say \"\"hi\"\"\",\"\"\n");
        assert_eq!(
            got,
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["x, y".to_string(), "two\nlines".to_string()],
                vec!["say \"hi\"".to_string(), String::new()],
            ]
        );
    }

    #[test]
    fn blank_lines_and_the_last_break_are_not_rows() {
        assert_eq!(rows("a\n\nb"), vec![vec!["a".to_string()], vec!["b".to_string()]]);
        assert_eq!(rows("a\n"), vec![vec!["a".to_string()]]);
        assert_eq!(rows(""), Vec::<Vec<String>>::new());
        // An empty quoted field is a field, and its line a row.
        assert_eq!(rows("\"\"\n"), vec![vec![String::new()]]);
    }

    #[test]
    fn a_stray_quote_and_a_bom_are_taken_in_stride() {
        assert_eq!(rows("\u{feff}5\" pipe,x"), vec![vec!["5\" pipe".to_string(), "x".to_string()]]);
        // Unterminated: the rest of the file is the field, not an error.
        assert_eq!(rows("\"open,x\ny"), vec![vec!["open,x\ny".to_string()]]);
    }

    #[test]
    fn the_separator_follows_the_extension_and_the_first_line() {
        assert_eq!(delimiter_for("tsv", "a,b"), '\t');
        assert_eq!(delimiter_for("csv", "a;b;c,d"), ';');
        assert_eq!(delimiter_for("csv", "a,b"), ',');
    }

    #[test]
    fn a_table_is_escaped_padded_and_capped() {
        let long = "word ".repeat(10);
        let html = render_table(&parse_delimited(&format!("h1,<h2>\n1,2,3,{long}\n"), ','), BIG).unwrap().unwrap();
        assert!(html.contains(&format!("<td class=\"w\">{long}</td>")), "{html}");
        assert!(html.contains("<th>h1</th><th>&lt;h2&gt;</th><th></th><th></th>"), "{html}");
        assert!(html.contains("<td class=\"n\">3</td>"), "{html}");
        assert!(render_table(&[], BIG).is_none());

        let mut many = vec![vec!["h".to_string()]];
        many.extend((0..MAX_TABLE_ROWS + 10).map(|i| vec![i.to_string()]));
        let html = render_table(&many, 1 << 30).unwrap().unwrap();
        assert_eq!(html.matches("<tr>").count(), MAX_TABLE_ROWS + 1);
    }

    #[test]
    fn json_is_laid_out_with_its_tokens_untouched() {
        let got = reformat_json("{\"b\":1,\"a\":[1, 2.50e3 ,{ }, []],\"s\":\"x\\\"}{\"}", BIG).unwrap();
        assert_eq!(
            got,
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2.50e3,\n    {},\n    []\n  ],\n  \"s\": \"x\\\"}{\"\n}\n"
        );
        assert_eq!(reformat_json("  42 ", BIG), Some("42\n".to_string()));
        // Every escape JSON has, copied as written.
        assert_eq!(reformat_json(r#"["\u00E9\n\/\\\"\b\f\r\t"]"#, BIG).unwrap(), "[\n  \"\\u00E9\\n\\/\\\\\\\"\\b\\f\\r\\t\"\n]\n");
    }

    #[test]
    fn records_in_a_row_stay_apart() {
        assert_eq!(reformat_json("{\"n\":1}\n{\"n\":2}\n", BIG).unwrap(), "{\n  \"n\": 1\n}\n{\n  \"n\": 2\n}\n");
        assert_eq!(reformat_json("1 2", BIG).unwrap(), "1\n2\n");
    }

    #[test]
    fn what_is_not_json_is_left_as_written() {
        for bad in ["", "  ", "{", "[1}", "{\"a\":1", "\"open", "// c\n{}", "{\"a\":1,}x]", "{\"a\":1,}", "[1, ]", "[1:2]", "1,2", "<x>"] {
            assert_eq!(reformat_json(bad, BIG), None, "{bad:?}");
        }
        // Tokens in an order JSON does not have, and words it does not have:
        // laid out, these would show what is not in the file.
        for bad in [
            "[1 2]", "{\"a\":1 \"b\":2}", "[\"a\" [1]]", "[,1]", "{\"a\"}", "{1:2}", "{\"a\" 1}", "[1,,2]",
            "hello world", "undefined", "NaN", "--1", "1.2.3", "tru", "+1", "01", "1.", ".5", "1e", "{\"a\":}",
            "[1]\u{a0}[2]", "[\"\\u12\"]", "[\"\\u12g4\"]", "[\"\\q\"]", "[\"a\tb\"]", "[\"a\nb\"]",
        ] {
            assert_eq!(reformat_json(bad, BIG), None, "{bad:?}");
        }
    }

    fn gz(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn a_gzip_opens_and_a_bomb_stops_at_the_cap() {
        assert_eq!(gunzip(&gz(b"line\n")[..], 100), Some((b"line\n".to_vec(), false)));
        // Members in a row read as one, as `zcat` reads them.
        let two = [gz(b"a\n"), gz(b"b\n")].concat();
        assert_eq!(gunzip(&two[..], 100), Some((b"a\nb\n".to_vec(), false)));
        assert_eq!(gunzip(&two[..], 3), Some((b"a\nb".to_vec(), true)));
        // Eight megabytes of zeros is a few kilobytes packed; the read stops one
        // byte past the cap.
        let bomb = gz(&vec![0u8; 8 << 20]);
        assert!(bomb.len() < 64 << 10);
        let (head, more) = gunzip(&bomb[..], 1 << 20).unwrap();
        assert!(more && head.len() == 1 << 20);
        assert_eq!(gunzip(&b"not gzip"[..], 100), None);
    }

    #[test]
    fn deep_or_long_json_stops_early() {
        // Twenty thousand levels is 40 KB that would lay out to 800 MB.
        let deep = format!("{}{}", "[".repeat(20_000), "]".repeat(20_000));
        assert_eq!(reformat_json(&deep, BIG), None);
        let ok = format!("{}{}", "[".repeat(MAX_JSON_DEPTH - 1), "[1]".to_string() + &"]".repeat(MAX_JSON_DEPTH - 1));
        assert!(reformat_json(&ok, BIG).is_some());
        // Wide rather than deep: stops at the cap, not after it.
        let wide = format!("[{}1]", "1,".repeat(1000));
        assert!(reformat_json(&wide, BIG).is_some());
        assert_eq!(reformat_json(&wide, 100), None);
    }

    #[test]
    fn a_number_is_not_a_date_a_phone_or_a_version() {
        for yes in ["3", "-1.5e-3", "1,234.5", "12%", "+7", "1_000", ".5"] {
            assert!(is_number(yes), "{yes}");
        }
        for no in ["2024-10-12", "1-800-555-0199", "12.34.56", "1e", "e5", "1,", ",1", "1.2,3", "5%%", "-", "1%2"] {
            assert!(!is_number(no), "{no}");
        }
    }

    #[test]
    fn a_table_too_wide_or_too_big_is_refused_early() {
        // One line of a million commas over five thousand short ones: padded,
        // that would be billions of cells. Refused on the width, before any.
        let text = format!("{}\n{}", ",".repeat(1_000_000), "x\n".repeat(5000));
        let rows = parse_delimited(&text, ',');
        assert_eq!(render_table(&rows, BIG), Some(Err(())));
        // Under the width but over the budget: stops at the row that passes it.
        let text = format!("{}\n{}", ",".repeat(MAX_TABLE_COLS - 1), "x\n".repeat(5000));
        let rows = parse_delimited(&text, ',');
        assert_eq!(render_table(&rows, BIG), Some(Err(())));
        assert!(matches!(render_table(&rows[..3], BIG), Some(Ok(_))));
    }
}
