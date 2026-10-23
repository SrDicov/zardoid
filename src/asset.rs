//! Clasificación y puntuación de assets de un release.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    TarGz,
    TarXz,
    TarZst,
    TarBz2,
    Zip,
    Deb,
    Rpm,
    AppImage,
    Gz,
    Xz,
    Zst,
    Bz2,
    Plain,
    Unknown,
}

impl Kind {
    pub fn from_name(name: &str) -> Kind {
        let n = name.to_ascii_lowercase();
        let n = n.strip_suffix(".sha256").unwrap_or(&n);
        if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
            Kind::TarGz
        } else if n.ends_with(".tar.xz") || n.ends_with(".txz") {
            Kind::TarXz
        } else if n.ends_with(".tar.zst") || n.ends_with(".tzst") {
            Kind::TarZst
        } else if n.ends_with(".tar.bz2") || n.ends_with(".tbz2") {
            Kind::TarBz2
        } else if n.ends_with(".appimage") {
            Kind::AppImage
        } else if n.ends_with(".zip") {
            Kind::Zip
        } else if n.ends_with(".deb") {
            Kind::Deb
        } else if n.ends_with(".rpm") {
            Kind::Rpm
        } else if n.ends_with(".gz") {
            Kind::Gz
        } else if n.ends_with(".xz") {
            Kind::Xz
        } else if n.ends_with(".zst") {
            Kind::Zst
        } else if n.ends_with(".bz2") {
            Kind::Bz2
        } else if n.ends_with(".tar") {
            Kind::Plain
        } else {
            Kind::Unknown
        }
    }
}
const REJECT: &[&str] = &[
    "windows",
    "win32",
    "win64",
    "win-",
    "mingw",
    ".exe",
    ".msi",
    "darwin",
    "macos",
    "mac-os",
    "osx",
    "apple",
    "dmg",
    "android",
    "apk",
    "ios",
    "freebsd",
    "openbsd",
    "netbsd",
    "dragonfly",
    "sunos",
    "aarch64",
    "arm64",
    "armv6",
    "armv7",
    "armhf",
    "armel",
    "arm",
    "i386",
    "i486",
    "i586",
    "i686",
    "386",
    "x86_32",
    "riscv",
    "mips",
    "ppc",
    "powerpc",
    "s390x",
    "loongarch",
    "sha256",
    "sha512",
    "sha1",
    "md5",
    "sha256sums",
    "checksums",
    ".sig",
    ".asc",
    ".pem",
    ".txt",
    ".json",
    ".yml",
    ".yaml",
    ".sbom",
    ".spdx",
    ".attestation",
    ".provenance",
];

/// Un nombre de asset termina dentro de un template bash: solo se aceptan
/// caracteres que no puedan salirse de las comillas ni del directorio.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '~' | '-'))
}

/// `Some(motivo)` si el asset no sirve para un paquete Linux x86_64.
pub fn reject_reason(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    REJECT.iter().copied().find(|t| n.contains(t))
}

/// Puntuación mayor = mejor. `None` si no se puede instalar como paquete.
pub fn score(kind: Kind, name: &str) -> Option<i32> {
    let base = match kind {
        Kind::AppImage => 95,
        Kind::TarGz => 90,
        Kind::TarXz => 88,
        Kind::TarZst => 86,
        Kind::TarBz2 => 80,
        Kind::Zip => 70,
        Kind::Deb => 60,
        Kind::Rpm => 55,
        Kind::Gz => 50,
        Kind::Xz => 48,
        Kind::Zst => 46,
        Kind::Bz2 => 44,
        Kind::Plain => 40,
        Kind::Unknown => return None,
    };
    let n = name.to_ascii_lowercase();
    let mut s = base;
    if n.contains("linux") {
        s += 10;
    }
    if n.contains("x86_64") || n.contains("amd64") {
        s += 15;
    }
    if n.contains("gnu") {
        s += 5;
    }
    if n.contains("musl") {
        s -= 5; // válido, pero obliga a archs musl
    }
    if n.contains("static") {
        s += 3;
    }
    for bad in [
        "installer",
        "setup",
        "portable",
        "debug",
        "dbg",
        "-src",
        "source",
    ] {
        if n.contains(bad) {
            s -= 40;
        }
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(Kind::from_name("x.tar.gz"), Kind::TarGz);
        assert_eq!(Kind::from_name("App.AppImage"), Kind::AppImage);
        assert_eq!(Kind::from_name("foo-linux-x86_64.gz"), Kind::Gz);
        assert_eq!(Kind::from_name("a.sha256"), Kind::Unknown);
    }

    #[test]
    fn rejects() {
        for n in [
            "app_Windows_x86_64.zip",
            "app-linux-arm64.tar.gz",
            "app-darwin-amd64.tar.gz",
            "SHA256SUMS",
            "app.exe",
            "app-i686.tar.gz",
        ] {
            assert!(reject_reason(n).is_some(), "{n} debería rechazarse");
        }
        for n in [
            "app-linux-x86_64.tar.gz",
            "App_1.0_amd64.deb",
            "app-musl-x86_64.gz",
            "tool.AppImage",
        ] {
            assert!(reject_reason(n).is_none(), "{n} no debería rechazarse");
        }
    }

    #[test]
    fn prefers_appimage_and_linux() {
        let a = score(Kind::AppImage, "App-x86_64.AppImage").unwrap();
        let b = score(Kind::TarGz, "app-linux-x86_64.tar.gz").unwrap();
        let c = score(Kind::TarGz, "app-linux-arm64.tar.gz").unwrap();
        assert!(b > a && a > c, "{a} {b} {c}");
        assert!(score(Kind::Unknown, "x").is_none());
    }
}
