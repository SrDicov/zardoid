//! Zardoid: de una URL de release a un template de Void Linux.

mod asset;
mod elf;
mod forge;
mod inspect;
mod template;
mod version;

use asset::Kind;
use forge::{Forge, Input, Release, Target};
use inspect::Inventory;
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Fail(i32, String);

impl From<String> for Fail {
    fn from(s: String) -> Self {
        Fail(2, s)
    }
}

type Res<T> = Result<T, Fail>;

struct Args {
    url: String,
    name: Option<String>,
    desc: Option<String>,
    license: Option<String>,
    maintainer: Option<String>,
    out: Option<PathBuf>,
    json: bool,
    force: bool,
    token: Option<String>,
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Token solo para su forge: no se manda el de GitHub a GitLab/Codeberg.
fn token_for(a: &Args, f: Forge) -> Option<String> {
    a.token.clone().or_else(|| {
        let key = match f {
            Forge::GitHub => "GITHUB_TOKEN",
            Forge::GitLab => "GITLAB_TOKEN",
            Forge::Codeberg => "CODEBERG_TOKEN",
        };
        std::env::var(key).ok().filter(|t| !t.is_empty())
    })
}

/// Comillas simples para rutas que vienen de un archivo upstream.
fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Valor de variable entre comillas dobles, escapando lo que bash expandiría.
/// Deja intacto `${version}` (lo inyecta zardoid, no el upstream).
fn dq(v: &str, version: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    let mut rest = v;
    while let Some(c) = rest.chars().next() {
        if !version.is_empty() && rest.starts_with("${version}") {
            out.push_str("${version}");
            rest = &rest["${version}".len()..];
            continue;
        }
        match c {
            '"' | '\\' | '$' | '`' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
        rest = &rest[c.len_utf8()..];
    }
    out.push('"');
    out
}

/// Igual, pero dejando `${version}` expandible: `'foo-'${version}'.bin'`.
fn shq_versioned(p: &Path, version: &str) -> String {
    let s = p.to_string_lossy();
    if !s.contains(version) {
        return shq(&s);
    }
    s.split(version)
        .map(shq)
        .collect::<Vec<_>>()
        .join("${version}")
}

fn usage() -> String {
    format!(
        "zardoid {VERSION} — template de Void Linux desde un release upstream

Uso: zardoid <url> [opciones]

  <url>                 repo (github/gitlab/codeberg) o asset directo
  --name <pkgname>      nombre del paquete (por defecto <repo>-bin)
  --desc <texto>        short_desc (por defecto, el del repo)
  --license <spdx>      licencia (por defecto, la detectada por el forge)
  --maintainer <quien>  \"Nombre <correo@dominio>\" (obligatorio)
  --out <dir>           escribe <dir>/srcpkgs/<pkg>/template
  --json                resumen JSON en stdout (requiere --out)
  --force               sobrescribe el template existente
  --token <token>       token de API (o GITHUB_TOKEN)

Requisitos: curl, jq, tar, sha256sum. Sin dependencias de Cargo."
    )
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        url: String::new(),
        name: None,
        desc: None,
        license: None,
        maintainer: std::env::var("ZARDOID_MAINTAINER").ok(),
        out: None,
        json: false,
        force: false,
        token: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |n: &str| it.next().ok_or_else(|| format!("{n} requiere un valor"));
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("zardoid {VERSION}");
                std::process::exit(0);
            }
            "--name" => a.name = Some(val("--name")?),
            "--desc" => a.desc = Some(val("--desc")?),
            "--license" => a.license = Some(val("--license")?),
            "--maintainer" => a.maintainer = Some(val("--maintainer")?),
            "--out" => a.out = Some(PathBuf::from(val("--out")?)),
            "--token" => a.token = Some(val("--token")?),
            "--json" => a.json = true,
            "--force" => a.force = true,
            s if s.starts_with("--") => return Err(format!("opción desconocida: {s}")),
            s => {
                if !a.url.is_empty() {
                    return Err("solo se acepta una URL".into());
                }
                a.url = s.to_string();
            }
        }
    }
    if a.url.is_empty() {
        return Err("falta la URL".into());
    }
    if a.json && a.out.is_none() {
        return Err("--json requiere --out".into());
    }
    Ok(a)
}

struct Meta {
    desc: String,
    license: String,
    homepage: String,
}

fn api_get(url: &str, token: Option<&str>, header: &str) -> Option<String> {
    let mut c = Command::new("curl");
    c.args(["-fsSL", "--max-time", "20", "--retry", "2"]);
    if let Some(t) = token {
        c.args(["-H", &format!("{header}: {t}")]);
    }
    c.arg(url);
    forge::run(c, None).ok()
}

fn jq_one(json: &str, filter: &str) -> String {
    let mut c = Command::new("jq");
    c.args(["-r", filter]);
    forge::run(c, Some(json))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn fetch_meta(t: &Target, token: Option<&str>) -> Meta {
    let (url, filter, header) = match t.forge {
        Forge::GitHub => (
            format!("https://api.github.com/repos/{}", t.repo),
            r#"[.description // "", (.license.spdx_id // ""), (.homepage // "")] | @tsv"#,
            "Authorization",
        ),
        Forge::GitLab => (
            format!(
                "https://gitlab.com/api/v4/projects/{}",
                t.repo.replace('/', "%2F")
            ),
            r#"[.description // "", "", (.web_url // "")] | @tsv"#,
            "PRIVATE-TOKEN",
        ),
        Forge::Codeberg => (
            format!("https://codeberg.org/api/v1/repos/{}", t.repo),
            r#"[.description // "", "", (.html_url // "")] | @tsv"#,
            "Authorization",
        ),
    };
    let empty = Meta {
        desc: String::new(),
        license: String::new(),
        homepage: t.homepage(),
    };
    let Some(body) = api_get(&url, token, header) else {
        return empty;
    };
    let line = jq_one(&body, filter);
    let mut f = line.split('\t');
    let desc = f.next().unwrap_or("").to_string();
    let license = map_license(f.next().unwrap_or(""));
    let home = f.next().unwrap_or("").trim();
    Meta {
        desc,
        license,
        homepage: if home.is_empty() {
            empty.homepage
        } else {
            home.to_string()
        },
    }
}

/// GitHub devuelve SPDX "cortos" (`GPL-3.0`); Void exige la forma moderna.
fn map_license(spdx: &str) -> String {
    match spdx.trim() {
        "" | "NOASSERTION" | "Other" => String::new(),
        "GPL-3.0" => "GPL-3.0-or-later".into(),
        "GPL-2.0" => "GPL-2.0-or-later".into(),
        "LGPL-3.0" => "LGPL-3.0-or-later".into(),
        "LGPL-2.1" => "LGPL-2.1-or-later".into(),
        "AGPL-3.0" => "AGPL-3.0-or-later".into(),
        s => s.to_string(),
    }
}

fn sanitize_pkg(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() || c == '+' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn short_desc(raw: &str, pkg: &str) -> String {
    let s = raw.trim();
    let s = s
        .strip_prefix(pkg)
        .map(|r| r.trim_start_matches([':', '-', ' ']))
        .unwrap_or(s);
    let mut out = String::new();
    for w in s.split_whitespace() {
        if !out.is_empty() && out.len() + w.len() + 1 > 72 {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    let mut out = out.trim_end_matches('.').to_string();
    if out.is_empty() {
        out = format!("Precompiled binaries for {}", pkg.trim_end_matches("-bin"));
    }
    let start = out.find(|c: char| c.is_alphanumeric()).unwrap_or(0);
    let mut out = out[start..].to_string();
    for art in ["The ", "A ", "An "] {
        if let Some(r) = out.strip_prefix(art) {
            out = r.to_string();
            break;
        }
    }
    if let Some(c) = out.chars().next() {
        if c.is_ascii_lowercase() {
            out = format!("{}{}", c.to_ascii_uppercase(), &out[1..]);
        }
    }
    out
}

fn guess_homepage(url: &str) -> String {
    if !url.starts_with("http") {
        return String::new();
    }
    for marker in [
        "/releases/download/",
        "/releases/tag/",
        "/archive/",
        "/-/releases/",
    ] {
        if let Some((base, _)) = url.split_once(marker) {
            return base.to_string();
        }
    }
    url.to_string()
}

fn main() {
    let code = match parse_args() {
        Err(e) => {
            eprintln!("zardoid: {e}\n\n{}", usage());
            1
        }
        Ok(args) => match run(args) {
            Ok(()) => 0,
            Err(Fail(code, msg)) => {
                eprintln!("zardoid: {msg}");
                code
            }
        },
    };
    std::process::exit(code);
}

fn run(a: Args) -> Res<()> {
    let mut warnings: Vec<String> = Vec::new();

    let input = forge::parse_input(&a.url).map_err(|e| Fail(1, e))?;
    let (release, meta, repo_name): (Release, Meta, Option<String>) = match input {
        Input::Repo(t) => {
            let tok = token_for(&a, t.forge);
            let rel = forge::latest(&t, tok.as_deref())?;
            let meta = fetch_meta(&t, tok.as_deref());
            let name = t.repo.rsplit('/').next().unwrap_or("pkg").to_string();
            (rel, meta, Some(name))
        }
        Input::Direct { url, hint_tag } => {
            let name = url.rsplit('/').next().unwrap_or("asset").to_string();
            let tag = hint_tag.or_else(|| forge::version_in(&url)).ok_or(Fail(
                1,
                "no se pudo deducir la versión del asset; usa un repo".into(),
            ))?;
            (
                Release {
                    tag,
                    prerelease: false,
                    assets: vec![forge::Asset {
                        name,
                        url: url.clone(),
                        size: 0,
                    }],
                },
                Meta {
                    desc: String::new(),
                    license: String::new(),
                    homepage: guess_homepage(&url),
                },
                None,
            )
        }
    };

    if release.prerelease {
        warnings.push(format!(
            "el release {} está marcado como prerelease",
            release.tag
        ));
    }

    let (asset, kind) = select_asset(&release, repo_name.is_none(), &mut warnings).ok_or(Fail(
        3,
        "no hay ningún asset Linux x86_64 utilizable en el release".into(),
    ))?;

    let version = version::from_tag(&release.tag)
        .or_else(|| forge::version_in(&asset.url))
        .ok_or(Fail(
            3,
            format!("tag no convertible a version=: {}", release.tag),
        ))?;

    let src = repo_name.unwrap_or_else(|| {
        asset
            .name
            .split(['-', '_', '.'])
            .find(|t| t.chars().any(|c| c.is_ascii_alphabetic()))
            .unwrap_or(&asset.name)
            .to_string()
    });
    let base = match &a.name {
        Some(n) => sanitize_pkg(n),
        None => format!("{}-bin", sanitize_pkg(&src)),
    };
    if base.trim_end_matches("-bin").is_empty() || base.len() > 64 {
        return Err(Fail(1, "nombre de paquete inválido".into()));
    }

    let maintainer = a
        .maintainer
        .clone()
        .filter(|m| m.contains('@') && m.contains('<') && m.contains('>'))
        .ok_or(Fail(
            1,
            "falta --maintainer \"Nombre <correo@dominio>\" (xlint no acepta placeholders)".into(),
        ))?;

    if maintainer.len() > 64 {
        return Err(Fail(1, "maintainer demasiado largo (máx. 64)".into()));
    }

    let license = a.license.clone().unwrap_or_else(|| meta.license.clone());
    if license.is_empty() {
        warnings.push("sin licencia detectada: pasa --license".into());
    }

    let dir_label = if base.is_empty() { &src } else { &base };
    let cache = inspect::cache_dir().join(format!("{dir_label}-{version}"));
    let file = cache.join(
        Path::new(&asset.name)
            .file_name()
            .ok_or(Fail(1, format!("nombre de asset inválido: {}", asset.name)))?,
    );
    if !file.exists() {
        eprintln!("==> descargando {}", asset.name);
    }
    inspect::download(&asset.url, &file)?;
    let checksum = inspect::sha256(&file)?;

    let tmp = std::env::temp_dir().join(format!("zardoid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let _guard = Cleanup(tmp.clone());

    let mut archs: Option<&'static str> = None;
    let mut functions: Vec<(String, String)> = Vec::new();
    let mut flags: Vec<(String, String)> = vec![
        ("nostrip".into(), "yes".into()),
        ("noshlibprovides".into(), "yes".into()),
    ];

    if kind == Kind::Zip {
        flags.push(("hostmakedepends".into(), "\"unzip\"".into()));
    }

    if kind == Kind::AppImage {
        functions.push(("post_extract".into(), image_copy()));
        functions.push(("do_install".into(), appimage_body(&base)));
        flags.push((
            "skip_extraction".into(),
            "\"${pkgname}-${version}.AppImage\"".into(),
        ));
        let lower = asset.name.to_ascii_lowercase();
        if ["x86_64", "x86-64", "amd64", "x64"]
            .iter()
            .any(|t| lower.contains(t))
        {
            archs = Some("x86_64");
        } else {
            warnings.push("AppImage sin arquitectura en el nombre: archs omitido".into());
        }
    } else {
        inspect::unpack(&file, kind, &tmp)?;
        let inv = inspect::inventory(&tmp);
        match inv.main_bin(&base) {
            Some(m) => {
                archs = m.info.archs();
                if archs.is_none() {
                    warnings.push("arquitectura ELF no mapeada: archs omitido".into());
                }
                if !m.info.pie {
                    flags.push(("nopie".into(), "yes".into()));
                }
            }
            None => warnings.push("no se encontró ningún ELF: revisa do_install".into()),
        }
        if inv.bins.is_empty() {
            warnings.push("el archivo no contiene binarios ELF".into());
        }
        if inv.files.is_empty() {
            warnings.push("el archivo extraído está vacío".into());
        }
        if inv.licenses.is_empty() {
            warnings.push("sin LICENSE/COPYING: añade vlicense a mano".into());
        }
        functions.push(("do_install".into(), generic_body(&base, &version, &inv)));
    }

    let mut distfiles = versioned_url(&asset.url, &version);
    if !distfiles.contains("${version}") {
        return Err(Fail(
            3,
            format!(
                "la URL del asset no contiene la versión: {asset_url}",
                asset_url = asset.url
            ),
        ));
    }
    if distfiles.contains("/latest/") {
        warnings.push("URL con /latest/: se congeló a los bytes actuales".into());
        distfiles = asset.url.clone();
    }
    // Nombre local del fichero: el de la URL puede estar codificado (GitLab) o
    // faltar (AppImage, cuyo extractor necesita un nombre predecible).
    if kind == Kind::AppImage {
        distfiles = format!("{distfiles}>${{pkgname}}-${{version}}.AppImage");
    } else {
        let url_base = asset.url.rsplit('/').next().unwrap_or("");
        if url_base != asset.name {
            distfiles = format!("{distfiles}>{}", asset.name);
        }
    }

    let desc = short_desc(&a.desc.clone().unwrap_or_else(|| meta.desc.clone()), &base);

    let mut fields: Vec<(String, String)> = Vec::new();
    if let Some(arch) = archs {
        fields.push(("archs".into(), format!("\"{arch}\"")));
    }
    fields.push(("short_desc".into(), dq(&desc, "")));
    fields.push(("maintainer".into(), dq(&maintainer, "")));
    if !license.is_empty() {
        fields.push(("license".into(), dq(&license, "")));
    }
    if !meta.homepage.is_empty() {
        fields.push(("homepage".into(), dq(&meta.homepage, "")));
    }
    fields.push(("distfiles".into(), dq(&distfiles, &version)));
    fields.push(("checksum".into(), checksum.clone()));
    fields.extend(flags);

    let text = template::render(&template::Spec {
        pkgname: base.clone(),
        version: version.clone(),
        revision: 1,
        fields,
        functions,
    });

    let errs = template::selfcheck(&text);
    if !errs.is_empty() {
        return Err(Fail(3, format!("template inválido: {}", errs.join("; "))));
    }
    verify(&text, &base)?;

    for n in template::long_lines(&text) {
        warnings.push(format!(
            "línea {n} supera 80 columnas (convención; xlint no lo exige)"
        ));
    }

    let warnings_json = warnings
        .iter()
        .map(|w| format!("\"{}\"", json_escape(w)))
        .collect::<Vec<_>>()
        .join(",");

    match &a.out {
        Some(dir) => {
            let dst = dir.join("srcpkgs").join(&base).join("template");
            if dst.exists() && !a.force {
                return Err(Fail(
                    4,
                    format!("{} ya existe (usa --force)", dst.display()),
                ));
            }
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p).map_err(|e| Fail(4, format!("mkdir: {e}")))?;
            }
            std::fs::write(&dst, &text).map_err(|e| Fail(4, format!("escribiendo: {e}")))?;
            if a.json {
                println!(
                    "{{\"pkgname\":\"{}\",\"version\":\"{}\",\"asset\":\"{}\",\"checksum\":\"{}\",\"template\":\"{}\",\"warnings\":[{}]}}",
                    json_escape(&base),
                    json_escape(&version),
                    json_escape(&asset.name),
                    checksum,
                    json_escape(&dst.display().to_string()),
                    warnings_json
                );
            } else {
                for w in &warnings {
                    eprintln!("advertencia: {w}");
                }
                eprintln!("==> escrito {}", dst.display());
            }
        }
        None => {
            for w in &warnings {
                eprintln!("advertencia: {w}");
            }
            print!("{text}");
        }
    }
    Ok(())
}

/// Sustituye la versión por `${version}` en todo el path (tag y fichero),
/// nunca en el host: `download/v1.2.3/foo-1.2.3.tar.gz` -> ambas.
fn versioned_url(url: &str, version: &str) -> String {
    let (prefix, path) = match url.split_once("://") {
        Some((scheme, rest)) => match rest.find('/') {
            Some(i) => (&url[..scheme.len() + 3 + i], &rest[i..]),
            None => return url.to_string(),
        },
        None => match url.find('/') {
            Some(i) => (&url[..i], &url[i..]),
            None => return url.to_string(),
        },
    };
    let mut out = path.replace(version, "${version}");
    out = out.replace(&version.replace('.', "%2E"), "${version}");
    format!("{prefix}{out}")
}

fn select_asset(
    r: &Release,
    direct: bool,
    warnings: &mut Vec<String>,
) -> Option<(forge::Asset, Kind)> {
    if direct {
        let a = &r.assets[0];
        if !asset::safe_name(&a.name) {
            warnings.push(format!("nombre de asset inseguro: {:?}", a.name));
            return None;
        }
        if let Some(reason) = asset::reject_reason(&a.name) {
            warnings.push(format!("el asset contiene '{reason}'"));
        }
        let mut k = Kind::from_name(&a.name);
        if k == Kind::Unknown && asset::platform_hint(&a.name) {
            k = Kind::Bin;
        }
        return Some((a.clone(), k));
    }
    let mut best: Option<(i32, u64, &forge::Asset, Kind)> = None;
    for a in &r.assets {
        if asset::reject_reason(&a.name).is_some() {
            continue;
        }
        if !asset::safe_name(&a.name) {
            warnings.push(format!("asset con nombre inseguro: {:?}", a.name));
            continue;
        }
        let mut k = Kind::from_name(&a.name);
        if k == Kind::Unknown && asset::platform_hint(&a.name) {
            // Ej.: `jq-linux-amd64`, `yt-dlp_linux`: ejecutable suelto.
            k = Kind::Bin;
        }
        if matches!(k, Kind::Deb | Kind::Rpm | Kind::Unknown) {
            continue;
        }
        let Some(s) = asset::score(k, &a.name) else {
            continue;
        };
        let better = match &best {
            None => true,
            Some((bs, bz, _, _)) => {
                s > *bs || (s == *bs && a.size > 0 && (*bz == 0 || a.size < *bz))
            }
        };
        if better {
            best = Some((s, a.size, a, k));
        }
    }
    best.map(|(_, _, a, k)| (a.clone(), k))
}

fn image_copy() -> String {
    "local f=\"${pkgname}-${version}.AppImage\"\n\
     cp \"${XBPS_SRCDISTDIR}/${pkgname}-${version}/${f}\" ."
        .to_string()
}

fn appimage_body(base: &str) -> String {
    format!(
        "local appimg=\"${{pkgname}}-${{version}}.AppImage\"\n\
         chmod +x \"${{appimg}}\"\n\
         \"${{appimg}}\" --appimage-extract >/dev/null\n\
         vmkdir opt/{base}\n\
         vcopy \"squashfs-root/*\" opt/{base}\n\
         vmkdir usr/bin\n\
         ln -sf \"/opt/{base}/AppRun\" \"${{DESTDIR}}/usr/bin/{stem}\"\n\
         local desk\n\
         desk=\"$(find squashfs-root -maxdepth 3 -name '*.desktop' | head -n1)\"\n\
         if [ -n \"$desk\" ]; then\n\
         \tvinstall \"$desk\" 644 usr/share/applications\n\
         fi\n\
         local icon\n\
         icon=\"$(find squashfs-root -name '*.png' -o -name '*.svg' | head -n1)\"\n\
         if [ -n \"$icon\" ]; then\n\
         \tvinstall \"$icon\" 644 usr/share/icons/hicolor/256x256/apps\n\
         fi",
        base = base,
        stem = base.trim_end_matches("-bin")
    )
}

fn generic_body(base: &str, version: &str, inv: &Inventory) -> String {
    let stem = base.trim_end_matches("-bin");
    let bin_name = |p: &Path| -> String {
        p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let install_bin = |p: &Path| -> String {
        let bn = bin_name(p);
        let keep = bn != stem && bn != base;
        if keep && bn.starts_with(stem) {
            format!("vbin {} '{}'", shq_versioned(p, version), stem)
        } else {
            format!("vbin {}", shq_versioned(p, version))
        }
    };
    let mut lines: Vec<String> = Vec::new();
    let mut bin_paths: Vec<&Path> = inv
        .bins
        .iter()
        .filter(|b| b.path.starts_with("bin/"))
        .map(|b| b.path.as_path())
        .collect();
    bin_paths.sort();
    if bin_paths.len() == 1 {
        lines.push(install_bin(bin_paths[0]));
    } else if bin_paths.is_empty() {
        match inv.main_bin(base) {
            Some(b) => lines.push(install_bin(&b.path)),
            None => lines.push(format!("vbin {stem}")),
        }
    } else {
        for p in &bin_paths {
            lines.push(format!("vbin {}", shq_versioned(p, version)));
        }
    }
    if let Some(d) = &inv.desktop {
        lines.push(format!(
            "vinstall {} 644 usr/share/applications",
            shq_versioned(d, version)
        ));
    }
    for i in &inv.icons {
        let s = i.to_string_lossy();
        let dir = if s.contains("scalable") || s.ends_with(".svg") {
            "usr/share/icons/hicolor/scalable/apps".to_string()
        } else if let Some(sz) = icon_size(&s) {
            format!("usr/share/icons/hicolor/{sz}/apps")
        } else {
            "usr/share/icons/hicolor/128x128/apps".to_string()
        };
        lines.push(format!(
            "vinstall {} 644 {}",
            shq_versioned(i, version),
            dir
        ));
    }
    if inv.man.len() > 5 {
        // `gh` trae ~300 páginas: una copia del árbol es una línea, no 300.
        lines.push("vcopy 'share/man/*' usr/share/man".to_string());
    } else {
        for m in &inv.man {
            lines.push(format!("vman {}", shq_versioned(m, version)));
        }
    }
    for c in &inv.completions {
        let s = c.to_string_lossy();
        let bn = c
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if s.contains("bash") || bn.ends_with(".bash") {
            let name = bn.strip_suffix(".bash").unwrap_or(&bn);
            lines.push(format!(
                "vinstall {} 644 usr/share/bash-completion/completions '{}'",
                shq_versioned(c, version),
                name
            ));
        } else if bn.ends_with(".ps1") || bn.ends_with(".pwsh") {
            continue;
        } else if s.contains("zsh") || bn.starts_with('_') {
            lines.push(format!(
                "vinstall {} 644 usr/share/zsh/site-functions",
                shq_versioned(c, version)
            ));
        } else {
            lines.push(format!(
                "vinstall {} 644 usr/share/fish/vendor_completions.d",
                shq_versioned(c, version)
            ));
        }
    }
    for l in &inv.licenses {
        lines.push(format!("vlicense {}", shq_versioned(l, version)));
    }
    lines.join("\n")
}

fn icon_size(path: &str) -> Option<String> {
    for part in path.split('/') {
        if let Some((w, h)) = part.split_once('x') {
            if w == h
                && !w.is_empty()
                && w.chars().all(|c| c.is_ascii_digit())
                && h.chars().all(|c| c.is_ascii_digit())
            {
                return Some(part.to_string());
            }
        }
    }
    None
}

/// `bash -n` + `format-template.py --check` (si `ZARDOID_FORMATTER` existe).
fn verify(text: &str, pkg: &str) -> Res<()> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("zardoid-check-{}-{stamp}", std::process::id()));
    let _clean = Cleanup(dir.clone());
    let path = dir.join("srcpkgs").join(pkg).join("template");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| Fail(3, e.to_string()))?;
    std::fs::write(&path, text).map_err(|e| Fail(3, e.to_string()))?;

    let out = Command::new("bash")
        .arg("-n")
        .arg(&path)
        .output()
        .map_err(|e| Fail(3, format!("bash no disponible: {e}")))?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(Fail(
            3,
            format!("bash -n: {}", String::from_utf8_lossy(&out.stderr).trim()),
        ));
    }

    if let Ok(f) = std::env::var("ZARDOID_FORMATTER") {
        if Path::new(&f).is_file() {
            let out = Command::new("python3")
                .arg(&f)
                .arg("--check")
                .arg(&path)
                .output()
                .map_err(|e| Fail(3, format!("format-template: {e}")))?;
            if !out.status.success() {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(Fail(
                    3,
                    format!(
                        "format-template --check falló: {}",
                        String::from_utf8_lossy(&out.stdout).trim()
                    ),
                ));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appimage_template_fits() {
        let text = template::render(&template::Spec {
            pkgname: "demo-bin".into(),
            version: "1.2.3".into(),
            revision: 1,
            fields: vec![
                ("archs".into(), "\"x86_64\"".into()),
                ("short_desc".into(), "\"Demo\"".into()),
                ("maintainer".into(), "\"T <t@e.org>\"".into()),
                ("license".into(), "\"MIT\"".into()),
                ("homepage".into(), "\"https://e.org\"".into()),
                (
                    "distfiles".into(),
                    "\"https://e.org/demo-1.2.3-x86_64.AppImage\"".into(),
                ),
                ("checksum".into(), "a".repeat(64)),
                (
                    "skip_extraction".into(),
                    "\"demo-1.2.3-x86_64.AppImage\"".into(),
                ),
            ],
            functions: vec![
                ("post_extract".into(), image_copy()),
                ("do_install".into(), appimage_body("demo-bin")),
            ],
        });
        for l in text.lines() {
            assert!(l.len() <= 80 || l.contains("://"), "{}", l);
        }
        assert!(template::selfcheck(&text).is_empty());
    }
}

#[cfg(test)]
mod sec_tests {
    use super::*;

    #[test]
    fn dq_escapa_expansiones() {
        assert_eq!(dq("Foo $(id)", ""), "\"Foo \\$(id)\"");
        assert_eq!(dq("a\"b`c\\d", ""), "\"a\\\"b\\`c\\\\d\"");
        assert_eq!(dq("x${version}y", "1.0"), "\"x${version}y\"");
        assert_eq!(dq("line\nbreak", ""), "\"line break\"");
    }

    #[test]
    fn version_solo_en_el_fichero() {
        assert_eq!(
            versioned_url("https://1.2.3.example.org/o/r/1.2.3/app-1.2.3.tgz", "1.2.3"),
            "https://1.2.3.example.org/o/r/${version}/app-${version}.tgz"
        );
        assert_eq!(
            versioned_url(
                "https://github.com/cli/cli/releases/download/v2.103.0/gh_2.103.0_linux_amd64.tar.gz",
                "2.103.0"
            ),
            "https://github.com/cli/cli/releases/download/v${version}/gh_${version}_linux_amd64.tar.gz"
        );
        assert_eq!(
            versioned_url("https://x/r/download/v2.0/App.AppImage", "2.0"),
            "https://x/r/download/v${version}/App.AppImage"
        );
        assert_eq!(
            versioned_url("https://x/p/1%2E4%2E0/f.tar.gz", "1.4.0"),
            "https://x/p/${version}/f.tar.gz"
        );
    }

    #[test]
    fn rutas_multibin_se_citan() {
        let inv = inspect::Inventory {
            files: vec![],
            bins: vec![
                inspect::BinEntry {
                    path: "bin/a one".into(),
                    info: crate::elf::Owned {
                        class: crate::elf::Class::Elf64,
                        machine: crate::elf::Machine::X86_64,
                        libc: crate::elf::Libc::Glibc,
                        pie: true,
                    },
                },
                inspect::BinEntry {
                    path: "bin/x'; touch /tmp/pwn2; :'".into(),
                    info: crate::elf::Owned {
                        class: crate::elf::Class::Elf64,
                        machine: crate::elf::Machine::X86_64,
                        libc: crate::elf::Libc::Glibc,
                        pie: true,
                    },
                },
            ],
            desktop: None,
            icons: vec![],
            man: vec![],
            completions: vec![],
            licenses: vec![],
        };
        let body = generic_body("demo-bin", "1.0", &inv);
        assert!(body.contains("vbin 'bin/a one'"), "{body}");
        assert!(body.contains("'\\''"), "comilla simple sin escapar: {body}");
        assert!(!body.contains("vbin bin/x';"), "inyección: {body}");
    }
}
