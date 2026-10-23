//! Lector mínimo de ELF: máquina e intérprete. Decide `archs` y `nostrip`
//! sin depender de `file(1)`. Solo little-endian (todos los assets que importan).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Elf32,
    Elf64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Machine {
    X86_64,
    Aarch64,
    I686,
    Arm,
    Riscv64,
    Other(u16),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Libc {
    Glibc,
    Musl,
    Static,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Owned {
    pub class: Class,
    pub machine: Machine,
    pub libc: Libc,
    /// `false` = ET_EXEC: binario no-PIE, el template necesita `nopie=yes`.
    pub pie: bool,
}

fn archs_for(machine: Machine, libc: Libc) -> Option<&'static str> {
    let (arch, is64) = match machine {
        Machine::X86_64 => ("x86_64", true),
        Machine::Aarch64 => ("aarch64", true),
        Machine::I686 => ("i686", false),
        Machine::Arm => ("armv7l", false),
        Machine::Riscv64 => ("riscv64", true),
        Machine::Other(_) => return None,
    };
    Some(match libc {
        // Un ELF glibc dinámico no corre en musl: sin comodín.
        Libc::Glibc => arch,
        // Un binario musl necesita los builders musl.
        Libc::Musl => match arch {
            "x86_64" => "*-musl",
            "aarch64" => "aarch64-musl",
            "i686" => "i686-musl",
            "armv7l" => "armv7l-musl",
            _ => "riscv64-musl",
        },
        // Estático o sin libc: vale para glibc y musl.
        Libc::Static => {
            let _ = is64;
            match arch {
                "x86_64" => "x86_64*",
                "aarch64" => "aarch64*",
                "i686" => "i686*",
                "armv7l" => "armv7l*",
                _ => "riscv64*",
            }
        }
    })
}

impl Owned {
    pub fn archs(&self) -> Option<&'static str> {
        archs_for(self.machine, self.libc)
    }
}

fn u16le(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

fn u32le(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn u64le(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off + 8)?.try_into().ok()?))
}

/// Lee cabecera + tabla de programas sin cargar el binario entero.
pub fn probe_path(p: &Path) -> Option<Owned> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut hdr = [0u8; 64];
    let n = f.read(&mut hdr).ok()?;
    if n < 52 || &hdr[..4] != b"\x7fELF" || hdr[5] != 1 {
        return None;
    }
    let is64 = hdr[4] == 2;
    let class = if is64 { Class::Elf64 } else { Class::Elf32 };
    let machine = match u16le(&hdr, 18)? {
        62 => Machine::X86_64,
        183 => Machine::Aarch64,
        3 => Machine::I686,
        40 => Machine::Arm,
        243 => Machine::Riscv64,
        m => Machine::Other(m),
    };
    let (phoff, phentsize, phnum) = if is64 {
        (
            u64le(&hdr, 32)?,
            u16le(&hdr, 54)? as usize,
            u16le(&hdr, 56)? as usize,
        )
    } else {
        (
            u32le(&hdr, 28)? as u64,
            u16le(&hdr, 42)? as usize,
            u16le(&hdr, 44)? as usize,
        )
    };
    let pie = u16le(&hdr, 16) == Some(3); // ET_DYN
    let static_ = Owned {
        class,
        machine,
        libc: Libc::Static,
        pie,
    };
    if phnum == 0 || phentsize < 32 || phnum > 256 {
        return Some(static_);
    }
    f.seek(SeekFrom::Start(phoff)).ok()?;
    let mut tab = vec![0u8; phentsize * phnum];
    let mut got = 0;
    while got < tab.len() {
        let r = f.read(&mut tab[got..]).ok()?;
        if r == 0 {
            break;
        }
        got += r;
    }
    for i in 0..phnum {
        let p = i * phentsize;
        if p + 40 > got || u32le(&tab, p)? != 3 {
            continue;
        }
        let (off, len) = if is64 {
            (u64le(&tab, p + 8)?, u64le(&tab, p + 32)?)
        } else {
            (u32le(&tab, p + 4)? as u64, u32le(&tab, p + 16)? as u64)
        };
        if len == 0 || len > 4096 {
            break;
        }
        f.seek(SeekFrom::Start(off)).ok()?;
        let mut buf = vec![0u8; len as usize];
        f.read_exact(&mut buf).ok()?;
        let s = buf.split(|&c| c == 0).next().unwrap_or(&buf);
        let s = String::from_utf8_lossy(s);
        let libc = if s.contains("ld-musl") {
            Libc::Musl
        } else if s.contains("ld-linux") || s.contains("ld.so") {
            Libc::Glibc
        } else {
            return Some(static_);
        };
        return Some(Owned {
            class,
            machine,
            libc,
            pie,
        });
    }
    Some(static_)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_elf() {
        let p = std::env::temp_dir().join(format!("zardoid-noelf-{}", std::process::id()));
        std::fs::write(&p, b"#!/bin/sh\n").unwrap();
        assert!(probe_path(&p).is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn current_binary() {
        let e = probe_path(Path::new("/proc/self/exe")).expect("ELF");
        assert_eq!(e.class, Class::Elf64);
        assert_eq!(e.machine, Machine::X86_64);
        assert_eq!(e.libc, Libc::Glibc);
        assert_eq!(e.archs(), Some("x86_64"));
        assert!(e.pie);
    }

    #[test]
    fn synthetic_static() {
        let mut b = vec![0u8; 128];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        b[18..20].copy_from_slice(&62u16.to_le_bytes());
        b[54..56].copy_from_slice(&56u16.to_le_bytes());
        let p = std::env::temp_dir().join(format!("zardoid-elf-{}", std::process::id()));
        std::fs::write(&p, &b).unwrap();
        let e = probe_path(&p).unwrap();
        assert_eq!(e.libc, Libc::Static);
        assert_eq!(e.archs(), Some("x86_64*"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn archs_matrix() {
        assert_eq!(archs_for(Machine::X86_64, Libc::Glibc), Some("x86_64"));
        assert_eq!(archs_for(Machine::X86_64, Libc::Musl), Some("*-musl"));
        assert_eq!(archs_for(Machine::X86_64, Libc::Static), Some("x86_64*"));
        assert_eq!(archs_for(Machine::Other(1), Libc::Glibc), None);
    }
}
