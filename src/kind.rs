//! What a binary file is, from its first bytes: the words a Source view puts on
//! its status line in place of "binary".
//!
//! Only what the head of a file says outright — a signature at a fixed place,
//! and for a picture the size its header states. No parser for any format:
//! where finding the answer would mean walking a structure past the head (a
//! PDF's page count, a zip's entries), the answer is left out, and a file this
//! does not know keeps the word it had.

/// How much of a file [`describe`] wants: enough for a JPEG whose size comes
/// after its EXIF block, which on a phone's photo often carries a thumbnail.
pub const HEAD: u64 = 64 * 1024;

/// The kind of file `head` starts, with what its header says that is worth
/// a line — "PNG · 240 × 160", "PDF 1.4", "ELF 64-bit x86-64" — or `None` for
/// a file nothing here recognises. `ext` settles the few signatures two
/// formats share.
pub fn describe(head: &[u8], ext: &str) -> Option<String> {
    let at = |i: usize, sig: &[u8]| head.get(i..i + sig.len()) == Some(sig);
    let sized = |name: &str, size: Option<(u32, u32)>| match size {
        Some((w, h)) if w > 0 && h > 0 => format!("{name} &middot; {w} &times; {h}"),
        _ => name.to_string(),
    };

    // Pictures, with the size their header states.
    if at(0, b"\x89PNG\r\n\x1a\n") {
        let size = at(12, b"IHDR").then(|| be32(head, 16).zip(be32(head, 20))).flatten();
        return Some(sized("PNG", size));
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return Some(sized("GIF", le16(head, 6).zip(le16(head, 8)).map(|(w, h)| (w as u32, h as u32))));
    }
    if at(0, b"\xff\xd8\xff") {
        return Some(sized("JPEG", jpeg_size(head)));
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Some(sized("WebP", webp_size(head)));
    }
    if at(0, b"BM") && ext == "bmp" {
        let size = le32(head, 18).zip(le32(head, 22)).map(|(w, h)| (w, (h as i32).unsigned_abs()));
        return Some(sized("BMP", size));
    }
    if at(0, b"II*\0") || at(0, b"MM\0*") {
        return Some("TIFF".into());
    }
    if at(0, b"\0\0\x01\0") && ext == "ico" {
        return Some("ICO".into());
    }

    // Documents and data.
    if at(0, b"%PDF-") {
        let version: String = head[5..].iter().take(4).take_while(|b| b.is_ascii_digit() || **b == b'.').map(|&b| b as char).collect();
        return Some(if version.is_empty() { "PDF".into() } else { format!("PDF {version}") });
    }
    if at(0, b"SQLite format 3\0") {
        return Some("SQLite 3".into());
    }

    // Programs.
    if at(0, b"\x7fELF") {
        return Some(elf(head));
    }
    if at(0, b"MZ") {
        return Some(pe(head));
    }
    if at(0, b"\xfe\xed\xfa\xce") || at(0, b"\xce\xfa\xed\xfe") {
        return Some("Mach-O 32-bit".into());
    }
    if at(0, b"\xfe\xed\xfa\xcf") || at(0, b"\xcf\xfa\xed\xfe") {
        return Some("Mach-O 64-bit".into());
    }
    if at(0, b"\xca\xfe\xba\xbe") {
        return Some(if ext == "class" { "Java class" } else { "Mach-O universal" }.into());
    }
    if at(0, b"\0asm") {
        return Some("WebAssembly".into());
    }
    if at(0, b"dex\n") {
        return Some("Android DEX".into());
    }

    // Archives and packed files.
    if at(0, b"\x1f\x8b") {
        return Some("gzip".into());
    }
    if at(0, b"PK\x03\x04") || at(0, b"PK\x05\x06") || at(0, b"PK\x07\x08") {
        return Some("zip".into());
    }
    if at(0, b"\xfd7zXZ\0") {
        return Some("xz".into());
    }
    if at(0, b"BZh") {
        return Some("bzip2".into());
    }
    if at(0, b"\x28\xb5\x2f\xfd") {
        return Some("zstd".into());
    }
    if at(0, b"7z\xbc\xaf\x27\x1c") {
        return Some("7-Zip".into());
    }
    if at(0, b"Rar!\x1a\x07") {
        return Some("RAR".into());
    }
    if at(257, b"ustar") {
        return Some("tar".into());
    }

    // Sound and film.
    if at(0, b"RIFF") && at(8, b"WAVE") {
        return Some("WAV".into());
    }
    if at(0, b"RIFF") && at(8, b"AVI ") {
        return Some("AVI".into());
    }
    if at(4, b"ftyp") {
        let brand = head.get(8..12).unwrap_or_default();
        return Some(
            match brand {
                b"avif" | b"avis" => "AVIF",
                b"heic" | b"heix" | b"mif1" | b"msf1" => "HEIF",
                b"qt  " => "QuickTime",
                b"M4A " | b"M4B " => "MPEG-4 audio",
                _ => "MPEG-4",
            }
            .into(),
        );
    }
    if at(0, b"\x1a\x45\xdf\xa3") {
        let webm = head[..head.len().min(64)].windows(4).any(|w| w == b"webm");
        return Some(if webm { "WebM" } else { "Matroska" }.into());
    }
    if at(0, b"OggS") {
        return Some("Ogg".into());
    }
    if at(0, b"fLaC") {
        return Some("FLAC".into());
    }
    // A bare MPEG frame sync is two bytes any file could start with: only
    // under its own name.
    if at(0, b"ID3") || (ext == "mp3" && head.len() > 1 && head[0] == 0xff && head[1] & 0xe6 == 0xe2) {
        return Some("MP3".into());
    }

    // Fonts.
    if at(0, b"wOFF") {
        return Some("WOFF".into());
    }
    if at(0, b"wOF2") {
        return Some("WOFF2".into());
    }
    if at(0, b"OTTO") {
        return Some("OpenType".into());
    }
    if at(0, b"\0\x01\0\0") && (ext == "ttf" || ext == "otf") {
        return Some("TrueType".into());
    }
    None
}

fn be16(b: &[u8], i: usize) -> Option<u16> {
    b.get(i..i + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

fn be32(b: &[u8], i: usize) -> Option<u32> {
    b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn le16(b: &[u8], i: usize) -> Option<u16> {
    b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn le32(b: &[u8], i: usize) -> Option<u32> {
    b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn le24(b: &[u8], i: usize) -> Option<u32> {
    b.get(i..i + 3).map(|s| u32::from_le_bytes([s[0], s[1], s[2], 0]))
}

/// A JPEG's size, from its first start-of-frame marker: the segments before it
/// are skipped by their lengths, so an EXIF block costs nothing but bytes.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xff {
            return None;
        }
        let marker = b[i + 1];
        // Fill bytes, and the markers that stand alone.
        if marker == 0xff {
            i += 1;
            continue;
        }
        if marker == 0x01 || (0xd0..=0xd9).contains(&marker) {
            i += 2;
            continue;
        }
        let len = be16(b, i + 2)? as usize;
        // Start of frame, all of them but the three codes that share the range.
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            let h = be16(b, i + 5)?;
            let w = be16(b, i + 7)?;
            return Some((w as u32, h as u32));
        }
        i += 2 + len;
    }
    None
}

/// A WebP's size, from whichever of its three first chunks it has.
fn webp_size(b: &[u8]) -> Option<(u32, u32)> {
    match b.get(12..16)? {
        b"VP8 " => Some(((le16(b, 26)? & 0x3fff) as u32, (le16(b, 28)? & 0x3fff) as u32)),
        b"VP8L" => {
            let bits = le32(b, 21)?;
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((le24(b, 24)? + 1, le24(b, 27)? + 1)),
        _ => None,
    }
}

/// "ELF 64-bit x86-64": the class and the machine, where the header says.
fn elf(b: &[u8]) -> String {
    let bits = match b.get(4) {
        Some(1) => " 32-bit",
        Some(2) => " 64-bit",
        _ => "",
    };
    let machine = match b.get(5) {
        Some(2) => be16(b, 18),
        _ => le16(b, 18),
    };
    let machine = match machine {
        Some(0x03) => " x86",
        Some(0x3e) => " x86-64",
        Some(0x28) => " ARM",
        Some(0xb7) => " AArch64",
        Some(0xf3) => " RISC-V",
        Some(0x08) => " MIPS",
        Some(0x14) => " PowerPC",
        Some(0x15) => " PowerPC64",
        Some(0x16) => " S/390",
        _ => "",
    };
    format!("ELF{bits}{machine}")
}

/// A Windows program and its machine, or the DOS stub that every one of them
/// starts with where the head does not reach the PE header.
fn pe(b: &[u8]) -> String {
    let Some(at) = le32(b, 0x3c).map(|n| n as usize) else {
        return "DOS executable".into();
    };
    if b.get(at..at + 4) != Some(b"PE\0\0") {
        return "DOS executable".into();
    }
    let machine = match le16(b, at + 4) {
        Some(0x8664) => " x86-64",
        Some(0x014c) => " x86",
        Some(0xaa64) => " ARM64",
        _ => "",
    };
    format!("Windows executable{machine}")
}

#[cfg(test)]
mod tests {
    use super::describe;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b
    }

    #[test]
    fn pictures_say_their_size() {
        assert_eq!(describe(&png(240, 160), "png").as_deref(), Some("PNG &middot; 240 &times; 160"));
        assert_eq!(describe(b"GIF89a\x10\0\x08\0", "gif").as_deref(), Some("GIF &middot; 16 &times; 8"));
        // An APP1 block ahead of the frame is skipped by its length.
        let mut jpeg = b"\xff\xd8\xff\xe1\0\x06Exif".to_vec();
        jpeg.extend(b"\xff\xc0\0\x11\x08\x02\x58\x03\x20");
        assert_eq!(describe(&jpeg, "jpg").as_deref(), Some("JPEG &middot; 800 &times; 600"));
        let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
        webp.extend([0x3f, 0x01, 0x00, 0xef, 0x00, 0x00]);
        assert_eq!(describe(&webp, "webp").as_deref(), Some("WebP &middot; 320 &times; 240"));
        // Cut short: the kind without a size.
        assert_eq!(describe(b"\xff\xd8\xff\xe1\0\x06", "jpg").as_deref(), Some("JPEG"));
        assert_eq!(describe(&png(240, 160)[..20], "png").as_deref(), Some("PNG"));
    }

    #[test]
    fn programs_say_their_machine() {
        let mut elf = b"\x7fELF\x02\x01\x01".to_vec();
        elf.resize(18, 0);
        elf.extend([0x3e, 0x00]);
        assert_eq!(describe(&elf, "").as_deref(), Some("ELF 64-bit x86-64"));
        let mut exe = b"MZ".to_vec();
        exe.resize(0x3c, 0);
        exe.extend(64u32.to_le_bytes());
        exe.resize(64, 0);
        exe.extend(b"PE\0\0\x64\x86");
        assert_eq!(describe(&exe, "exe").as_deref(), Some("Windows executable x86-64"));
        assert_eq!(describe(b"MZ", "exe").as_deref(), Some("DOS executable"));
        // One signature, two formats: the name settles it.
        assert_eq!(describe(b"\xca\xfe\xba\xbe", "class").as_deref(), Some("Java class"));
        assert_eq!(describe(b"\xca\xfe\xba\xbe", "").as_deref(), Some("Mach-O universal"));
    }

    #[test]
    fn documents_archives_and_the_unknown() {
        assert_eq!(describe(b"%PDF-1.7\n%", "pdf").as_deref(), Some("PDF 1.7"));
        assert_eq!(describe(b"\x1f\x8b\x08", "gz").as_deref(), Some("gzip"));
        assert_eq!(describe(b"PK\x03\x04", "docx").as_deref(), Some("zip"));
        let mut tar = vec![0u8; 257];
        tar.extend(b"ustar\0");
        assert_eq!(describe(&tar, "tar").as_deref(), Some("tar"));
        assert_eq!(describe(b"\0\0\0\x20ftypheic", "heic").as_deref(), Some("HEIF"));
        assert_eq!(describe(b"\x1a\x45\xdf\xa3\x01\0\0\0\0\0\0\x1f\x42\x82\x84webm", "webm").as_deref(), Some("WebM"));
        // Plain bytes, and a BMP signature on a file that is not one.
        assert_eq!(describe(b"\0\x01\x02\x03", "bin"), None);
        assert_eq!(describe(b"BM\0\0", "dat"), None);
    }
}
