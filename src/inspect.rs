//! Descarga, hash, desempaquetado e inventario del asset.

use crate::asset::Kind;
use crate::elf::{self, Libc};
use crate::forge::run;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(d).join("zardoid");
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".cache/zardoid")
}

pub fn download(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p).map_err(|e| format!("creando {}: {e}", p.display()))?;
    }
    if dest.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(());
    }
    let tmp = dest.with_extension("part");
    let mut c = Command::new("curl");
    c.args(["-fsSL", "--retry", "3", "--connect-timeout", "20", "-o"])
        .arg(&tmp)
        .arg(url);
    if let Err(e) = run(c, None) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("descarga fallida: {e}"));
    }
    fs::rename(&tmp, dest).map_err(|e| format!("renombrando descarga: {e}"))
}

pub fn sha256(path: &Path) -> Result<String, String> {
    let attempts: [&[&str]; 3] = [
        &["sha256sum"],
        &["shasum", "-a", "256"],
        &["openssl", "dgst", "-sha256"],
    ];
    let mut last = String::new();
    for (i, argv) in attempts.iter().enumerate() {
        let mut c = Command::new(argv[0]);
        c.args(&argv[1..]);
        c.arg(path);
        match run(c, None) {
            Ok(out) => {
                let h = out
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_start_matches("SHA256(")
                    .to_string();
                if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Ok(h.to_ascii_lowercase());
                }
                last = format!("salida inesperada de {}", argv[0]);
            }
            Err(e) => {
                last = e;
                if i + 1 == attempts.len() {
                    break;
                }
            }
        }
    }
    Err(format!("no se pudo calcular sha256: {last}"))
}

fn decompress(bin: &str, args: &[&str], src: &Path, out: &Path) -> Result<(), String> {
    let mut c = Command::new(bin);
    c.args(args).arg(src).stdout(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("{bin}: {e}"))?;
    let mut f = fs::File::create(out).map_err(|e| format!("creando {}: {e}", out.display()))?;
    std::io::copy(child.stdout.as_mut().ok_or("sin stdout")?, &mut f)
        .map_err(|e| format!("descomprimiendo: {e}"))?;
    let st = child.wait().map_err(|e| format!("{bin}: {e}"))?;
    if !st.success() {
        return Err(format!("{bin} falló sobre {}", src.display()));
    }
    Ok(())
}

/// Rechaza miembros con ruta absoluta o `..` antes de extraer: un tarball
/// hostil no debe escribir fuera del directorio temporal.
fn safe_members(list: &str) -> Result<(), String> {
    for line in list.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if l.starts_with('/') || l.split('/').any(|c| c == "..") {
            return Err(format!("archivo con ruta insegura: {l}"));
        }
    }
    Ok(())
}

fn precheck(archive: &Path, kind: Kind) -> Result<(), String> {
    let out = match kind {
        Kind::Zip => {
            let mut c = Command::new("unzip");
            c.args(["-Z1"]).arg(archive);
            run(c, None)?
        }
        Kind::TarGz | Kind::TarXz | Kind::TarZst | Kind::TarBz2 | Kind::Plain => {
            let mut c = Command::new("tar");
            c.arg("-tf").arg(archive);
            run(c, None)?
        }
        // Un binario comprimido no tiene miembros que validar.
        _ => return Ok(()),
    };
    safe_members(&out)
}

pub fn unpack(archive: &Path, kind: Kind, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("creando {}: {e}", dir.display()))?;
    precheck(archive, kind)?;
    // ponytail: un miembro symlink puede apuntar fuera del tmp y un tar viejo
    // escribiría a través de él. Techo: extracción en el host. Mejora: extraer
    // con `unshare -r` o parsear el archivo en Rust si esto se expone en red.
    let name = archive
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    match kind {
        Kind::TarGz | Kind::TarXz | Kind::TarBz2 | Kind::Plain => {
            let mut c = Command::new("tar");
            c.arg("-xf").arg(archive).arg("-C").arg(dir);
            run(c, None).map_err(|e| format!("tar: {e}"))?;
        }
        Kind::TarZst => {
            let mut c = Command::new("tar");
            c.arg("-xf").arg(archive).arg("-C").arg(dir);
            if run(c, None).is_err() {
                // tar sin soporte zstd: descomprimir a un tar temporal.
                let tmp = dir.join("._zardoid.tar");
                decompress("zstd", &["-dc"], archive, &tmp)?;
                let mut c = Command::new("tar");
                c.arg("-xf").arg(&tmp).arg("-C").arg(dir);
                run(c, None).map_err(|e| format!("tar (zstd): {e}"))?;
                let _ = fs::remove_file(tmp);
            }
        }
        Kind::Zip => {
            let mut c = Command::new("unzip");
            c.args(["-q", "-o"]).arg(archive).arg("-d").arg(dir);
            run(c, None).map_err(|e| format!("unzip: {e}"))?;
        }
        Kind::Gz | Kind::Xz | Kind::Zst | Kind::Bz2 => {
            let stem = name
                .rsplit_once('.')
                .map(|(s, _)| s)
                .unwrap_or(&name)
                .to_string();
            let out = dir.join(stem);
            let (bin, args) = match kind {
                Kind::Gz => ("gzip", &["-dc"][..]),
                Kind::Xz => ("xz", &["-dc"][..]),
                Kind::Zst => ("zstd", &["-dc"][..]),
                _ => ("bzip2", &["-dc"][..]),
            };
            decompress(bin, args, archive, &out)?;
        }
        Kind::AppImage => {}
        Kind::Deb | Kind::Rpm | Kind::Unknown => {
            return Err(format!("formato no soportado en v1: {name}"));
        }
    }
    Ok(())
}

pub struct BinEntry {
    pub path: PathBuf,
    pub info: elf::Owned,
}

pub struct Inventory {
    pub files: Vec<PathBuf>,
    pub bins: Vec<BinEntry>,
    pub desktop: Option<PathBuf>,
    pub icons: Vec<PathBuf>,
    pub man: Vec<PathBuf>,
    pub completions: Vec<PathBuf>,
    pub licenses: Vec<PathBuf>,
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let dir = root.join(rel);
    let Ok(rd) = fs::read_dir(&dir) else { return };
    for e in rd.flatten() {
        let p = rel.join(e.file_name());
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(root, &p, out);
        } else if ft.is_file() {
            out.push(p);
        }
    }
}

/// `None` si la extracción dejó un único directorio raíz que xbps-src hoistea.
fn inner_root(dir: &Path) -> PathBuf {
    let entries: Vec<_> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with("._"))
                .collect()
        })
        .unwrap_or_default();
    if entries.len() == 1 && entries[0].file_type().map(|t| t.is_dir()).unwrap_or(false) {
        entries[0].path()
    } else {
        dir.to_path_buf()
    }
}

pub fn inventory(dir: &Path) -> Inventory {
    let root = inner_root(dir);
    let mut files = Vec::new();
    walk(&root, Path::new(""), &mut files);

    let mut bins = Vec::new();
    for f in &files {
        if let Some(info) = elf::probe_path(&root.join(f)) {
            bins.push(BinEntry {
                path: f.clone(),
                info,
            });
        }
    }
    let is = |f: &Path, ext: &str| {
        f.extension()
            .map(|e| e.eq_ignore_ascii_case(ext))
            .unwrap_or(false)
    };
    let desktop = files.iter().find(|f| is(f, "desktop")).cloned();
    let icons = files
        .iter()
        .filter(|f| is(f, "png") || is(f, "svg"))
        .cloned()
        .collect();
    let man = files
        .iter()
        .filter(|f| {
            let s = f.to_string_lossy();
            if s.contains("/man/") || s.starts_with("man/") {
                return true;
            }
            let n = f
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            let n = n.strip_suffix(".gz").unwrap_or(&n);
            n.rsplit_once('.')
                .map(|(_, e)| e.len() == 1 && e.chars().all(|c| c.is_ascii_digit() && c != '0'))
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    let completions = files
        .iter()
        .filter(|f| {
            let s = f.to_string_lossy();
            s.contains("bash-completion")
                || s.contains("zsh/site-functions")
                || s.contains("fish/vendor_completions")
                || s.contains("/complete/")
                || s.starts_with("complete/")
        })
        .cloned()
        .collect();
    let licenses = files
        .iter()
        .filter(|f| {
            let n = f
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_uppercase())
                .unwrap_or_default();
            n.starts_with("LICENSE")
                || n.starts_with("COPYING")
                || n.starts_with("COPYRIGHT")
                || n.starts_with("UNLICENSE")
                || n == "LICENCE"
        })
        .cloned()
        .collect();

    Inventory {
        files,
        bins,
        desktop,
        icons,
        man,
        completions,
        licenses,
    }
}

impl Inventory {
    /// Binario principal: el que se llama como el paquete, o el primero de `bin/`.
    pub fn main_bin(&self, base: &str) -> Option<&BinEntry> {
        self.bins.iter().max_by_key(|b| {
            let n = b
                .path
                .file_name()
                .map(|x| x.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let mut s = 0;
            if n == base {
                s += 100;
            }
            if n == format!("{base}-bin") {
                s += 90;
            }
            if b.path.starts_with("bin/") {
                s += 50;
            }
            if b.info.libc != Libc::Static {
                s += 5;
            }
            s
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("zardoid-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn unpack_tar_and_inventory() {
        let t = tmp();
        let src = t.join("pkg/bin");
        fs::create_dir_all(&src).unwrap();
        let exe = src.join("demo");
        fs::copy("/proc/self/exe", &exe).unwrap();
        let mut d = fs::File::create(t.join("pkg/demo.desktop")).unwrap();
        d.write_all(b"[Desktop Entry]\nName=Demo\n").unwrap();
        fs::write(t.join("pkg/LICENSE"), "MIT").unwrap();
        let ar = t.join("demo.tar.gz");
        let st = Command::new("tar")
            .arg("-czf")
            .arg(&ar)
            .arg("-C")
            .arg(&t)
            .arg("pkg")
            .status()
            .unwrap();
        assert!(st.success());

        let out = t.join("x");
        unpack(&ar, Kind::TarGz, &out).unwrap();
        let inv = inventory(&out);
        assert_eq!(inv.bins.len(), 1);
        assert_eq!(inv.bins[0].info.machine, crate::elf::Machine::X86_64);
        assert_eq!(inv.desktop.as_deref(), Some(Path::new("demo.desktop")));
        assert_eq!(inv.licenses.len(), 1);
        assert!(inv.main_bin("demo").is_some());
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn rejects_unsafe_members() {
        assert!(safe_members("bin/ok\n./share/x\n").is_ok());
        assert!(safe_members("../.ssh/authorized_keys\n").is_err());
        assert!(safe_members("bin/../../etc/passwd\n").is_err());
        assert!(safe_members("/etc/shadow\n").is_err());
    }

    #[test]
    fn sha_of_known_content() {
        let t = tmp();
        let p = t.join("f");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_dir_all(&t);
    }
}
