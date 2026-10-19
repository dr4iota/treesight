//! Which permission bits a copy takes when it crosses between this device and
//! the tree — Upload and Download. The tree may be on another machine, of
//! another OS and CPU, which nothing here knows more of than
//! [`crate::Vfs::on_this_device`] says. The read and write bits travel as they
//! are; an `x` travels only on a file that can be run, a script or a program;
//! and setuid, setgid and sticky never do.
//!
//! What a file *is* comes from its first bytes, as everywhere else here
//! (`util::looks_binary`, a shebang for the highlighter), not from its name: a
//! program on Unix has no extension, and a `.py` without `#!` does not run as
//! `./x.py` anyway. Reading [`HEAD`] bytes is the whole cost.

use std::io::{self, Read};

/// How much of a file's start [`kind`] looks at: an ELF header's machine, and
/// two architectures of a universal Mach-O.
pub const HEAD: usize = 64;

/// What a file's first bytes say about running it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `#!`: runs wherever its interpreter does.
    Script,
    /// An ELF or Mach-O program; `here` when its format and CPU are this
    /// machine's, so a copy here would run.
    Program { here: bool },
    /// A Windows program, which no `x` makes run.
    Windows,
    /// Anything else.
    Data,
}

/// Up to [`HEAD`] bytes from the start of `from`, which reads on from there.
pub fn head(from: &mut dyn Read) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(HEAD);
    from.take(HEAD as u64).read_to_end(&mut out)?;
    Ok(out)
}

pub fn kind(head: &[u8]) -> Kind {
    if head.starts_with(b"#!") {
        return Kind::Script;
    }
    if head.starts_with(b"\x7fELF") {
        let machine = match (head.get(5), head.get(18..20)) {
            (Some(2), Some(m)) => Some(u16::from_be_bytes([m[0], m[1]])),
            (_, Some(m)) => Some(u16::from_le_bytes([m[0], m[1]])),
            _ => None,
        };
        let here = cfg!(target_os = "linux") && machine.is_some() && machine == ELF_MACHINE;
        return Kind::Program { here };
    }
    if let Some(here) = mach_o(head) {
        return Kind::Program { here: cfg!(target_os = "macos") && here };
    }
    if head.starts_with(b"MZ") {
        return Kind::Windows;
    }
    Kind::Data
}

/// Whether `head` is a Mach-O program, and if so whether one of its CPUs is
/// this machine's. A universal binary's magic is also a Java class file's;
/// the count of architectures after it tells them apart.
fn mach_o(head: &[u8]) -> Option<bool> {
    let u32_at = |at: usize, be: bool| {
        head.get(at..at + 4).map(|b| {
            let b = [b[0], b[1], b[2], b[3]];
            if be { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }
        })
    };
    let ours = |cpu: Option<u32>| cpu.is_some() && cpu == MACH_CPU;
    match head.get(..4)? {
        [0xcf | 0xce, 0xfa, 0xed, 0xfe] => Some(ours(u32_at(4, false))),
        [0xfe, 0xed, 0xfa, 0xcf | 0xce] => Some(ours(u32_at(4, true))),
        [0xca, 0xfe, 0xba, 0xbe | 0xbf] => {
            let n = u32_at(4, true)?;
            if n == 0 || n > 30 {
                return None;
            }
            let entry = if head[3] == 0xbf { 32 } else { 20 };
            Some((0..n as usize).any(|i| ours(u32_at(8 + i * entry, true))))
        }
        _ => None,
    }
}

/// This machine's CPU as an ELF header names it.
const ELF_MACHINE: Option<u16> = if cfg!(target_arch = "x86_64") {
    Some(62)
} else if cfg!(target_arch = "aarch64") {
    Some(183)
} else if cfg!(target_arch = "x86") {
    Some(3)
} else if cfg!(target_arch = "arm") {
    Some(40)
} else if cfg!(target_arch = "riscv64") {
    Some(243)
} else {
    None
};

/// This machine's CPU as a Mach-O header names it.
const MACH_CPU: Option<u32> = if cfg!(target_arch = "x86_64") {
    Some(0x0100_0007)
} else if cfg!(target_arch = "aarch64") {
    Some(0x0100_000c)
} else {
    None
};

/// Download: the mode a copy here of a file in the tree takes. Its read and
/// write bits, and its `x` where it has one and the file runs here — a script,
/// or a program for this machine. A download never adds an `x` the tree's file
/// did not have.
pub fn downloaded(mode: u32, head: &[u8]) -> u32 {
    let x = match kind(head) {
        Kind::Script | Kind::Program { here: true } => mode & 0o111,
        _ => 0,
    };
    mode & 0o666 | x
}

/// Upload: the mode a new file in the tree asks for, which the tree's own
/// umask then trims. `source` is what the file has where that is real — a Unix
/// desktop's own disk — and `None` from Windows, a phone, or a disk that
/// makes one up ([`believable`]). `tree_here` is the tree's
/// [`crate::Vfs::on_this_device`].
///
/// With a mode: its read and write bits, and its `x` where the file is a
/// script or a program that runs where the tree is. In a tree on this device
/// that is a program for this machine; in one elsewhere, any program, since
/// that machine is not known here and a program is copied there to be run.
/// Without a mode: a script or such a program asks for every bit, so an `x`
/// beside each `r` (755 under umask 022), and anything else asks for nothing,
/// which is the tree's default.
pub fn uploaded(source: Option<u32>, head: &[u8], tree_here: bool) -> Option<u32> {
    let runs = match kind(head) {
        Kind::Script => true,
        Kind::Program { here } => here || !tree_here,
        Kind::Windows | Kind::Data => false,
    };
    match source {
        Some(m) => Some(m & 0o666 | if runs { m & 0o111 } else { 0 }),
        None => runs.then_some(0o777),
    }
}

/// Whether a file's mode could be one somebody chose. Every bit of `777` is
/// what a disk with no modes of its own hands every file — FAT and exFAT on
/// a Mac, NTFS through `ntfs-3g`, a Windows drive under WSL — and nobody sets
/// it on purpose.
pub fn believable(mode: u32) -> bool {
    mode & 0o777 != 0o777
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elf(machine: u16) -> Vec<u8> {
        let mut h = b"\x7fELF\x02\x01\x01".to_vec();
        h.resize(18, 0);
        h.extend(machine.to_le_bytes());
        h.resize(HEAD, 0);
        h
    }

    fn mach(cpu: u32) -> Vec<u8> {
        let mut h = vec![0xcf, 0xfa, 0xed, 0xfe];
        h.extend(cpu.to_le_bytes());
        h
    }

    fn fat(cpus: &[u32]) -> Vec<u8> {
        let mut h = vec![0xca, 0xfe, 0xba, 0xbe];
        h.extend((cpus.len() as u32).to_be_bytes());
        for cpu in cpus {
            h.extend(cpu.to_be_bytes());
            h.extend([0; 16]);
        }
        h
    }

    const X86_64_ELF: u16 = 62;
    const AARCH64_ELF: u16 = 183;
    const X86_64_MACH: u32 = 0x0100_0007;
    const ARM64_MACH: u32 = 0x0100_000c;

    /// The first bytes, not the name: a shebang, a program for this machine
    /// or another, a Windows one, and the rest — a Java class included, whose
    /// magic a universal Mach-O shares.
    #[test]
    fn a_file_is_known_by_its_first_bytes() {
        assert_eq!(kind(b"#!/bin/sh\necho hi\n"), Kind::Script);
        assert_eq!(kind(b"echo hi\n"), Kind::Data);
        assert_eq!(kind(b""), Kind::Data);
        assert_eq!(kind(b"MZ\x90\0"), Kind::Windows);
        let linux_here = cfg!(all(target_os = "linux", target_arch = "x86_64"));
        assert_eq!(kind(&elf(X86_64_ELF)), Kind::Program { here: linux_here });
        let pi_here = cfg!(all(target_os = "linux", target_arch = "aarch64"));
        assert_eq!(kind(&elf(AARCH64_ELF)), Kind::Program { here: pi_here });
        assert_eq!(kind(b"\x7fELF"), Kind::Program { here: false }, "too short to name a CPU");
        let mac_here = cfg!(all(target_os = "macos", target_arch = "aarch64"));
        assert_eq!(kind(&mach(ARM64_MACH)), Kind::Program { here: mac_here });
        let either = cfg!(target_os = "macos") && cfg!(any(target_arch = "aarch64", target_arch = "x86_64"));
        assert_eq!(kind(&fat(&[X86_64_MACH, ARM64_MACH])), Kind::Program { here: either });
        let java = [0xca, 0xfe, 0xba, 0xbe, 0x00, 0x00, 0x00, 0x41];
        assert_eq!(kind(&java), Kind::Data);
    }

    /// Down: read and write as the tree has them, an `x` only where the file
    /// runs here and the tree's had one, and never setuid.
    #[test]
    fn a_download_keeps_x_only_where_it_runs_here() {
        assert_eq!(downloaded(0o755, b"#!/bin/sh\n"), 0o755);
        assert_eq!(downloaded(0o700, b"#!/bin/sh\n"), 0o700);
        assert_eq!(downloaded(0o644, b"#!/bin/sh\n"), 0o644, "never added");
        assert_eq!(downloaded(0o4755, b"#!/bin/sh\n"), 0o755, "no setuid");
        assert_eq!(downloaded(0o777, b"notes\n"), 0o666);
        assert_eq!(downloaded(0o755, b"MZ"), 0o644);
        let here = cfg!(all(target_os = "linux", target_arch = "x86_64"));
        assert_eq!(downloaded(0o755, &elf(X86_64_ELF)), if here { 0o755 } else { 0o644 });
        let pi = cfg!(all(target_os = "linux", target_arch = "aarch64"));
        assert_eq!(downloaded(0o755, &elf(AARCH64_ELF)), if pi { 0o755 } else { 0o644 });
    }

    /// Up, with a real mode: its own `x` on a script or a program that runs
    /// where the tree is, none on the rest. Without one: an `x` asked for on
    /// those, and nothing asked on the rest. A tree elsewhere is a machine not
    /// known here, so any program runs there; one on this device is this one.
    #[test]
    fn an_upload_keeps_or_asks_for_x_on_what_runs() {
        for here in [false, true] {
            assert_eq!(uploaded(Some(0o700), b"#!/bin/sh\n", here), Some(0o700));
            assert_eq!(uploaded(Some(0o755), b"notes\n", here), Some(0o644));
            assert_eq!(uploaded(Some(0o755), b"MZ", here), Some(0o644));
            assert_eq!(uploaded(Some(0o600), b"#!/bin/sh\n", here), Some(0o600), "never added");
            assert_eq!(uploaded(Some(0o4755), b"#!/bin/sh\n", here), Some(0o755), "no setuid");
            assert_eq!(uploaded(None, b"#!/usr/bin/env python3\n", here), Some(0o777));
            assert_eq!(uploaded(None, b"notes\n", here), None);
            assert_eq!(uploaded(None, b"MZ", here), None);
        }
        assert_eq!(uploaded(Some(0o755), &elf(AARCH64_ELF), false), Some(0o755), "any CPU's, elsewhere");
        assert_eq!(uploaded(None, &elf(AARCH64_ELF), false), Some(0o777));
        let pi = cfg!(all(target_os = "linux", target_arch = "aarch64"));
        assert_eq!(uploaded(Some(0o755), &elf(AARCH64_ELF), true), Some(if pi { 0o755 } else { 0o644 }), "this one's, here");
        assert_eq!(uploaded(None, &elf(AARCH64_ELF), true), if pi { Some(0o777) } else { None });
    }

    #[test]
    fn every_bit_is_a_made_up_mode() {
        assert!(!believable(0o777));
        assert!(!believable(0o100777));
        assert!(believable(0o755) && believable(0o775) && believable(0o555));
    }

    #[test]
    fn head_reads_on_from_where_it_stopped() {
        let mut from: &[u8] = &[7u8; 100];
        let h = head(&mut from).unwrap();
        assert_eq!(h.len(), HEAD);
        assert_eq!(from.len(), 100 - HEAD);
        let mut short: &[u8] = b"#!";
        assert_eq!(head(&mut short).unwrap(), b"#!");
    }
}
