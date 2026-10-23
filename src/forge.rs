//! Identificación de forge, consulta de releases y modelo de datos.
//! HTTP con `curl`, JSON con `jq`: cero crates, cero TLS propio.

use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub tag: String,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forge {
    GitHub,
    GitLab,
    Codeberg,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub forge: Forge,
    pub repo: String,
    pub tag: Option<String>,
}

impl Target {
    pub fn homepage(&self) -> String {
        let host = match self.forge {
            Forge::GitHub => "https://github.com",
            Forge::GitLab => "https://gitlab.com",
            Forge::Codeberg => "https://codeberg.org",
        };
        format!("{host}/{}", self.repo)
    }
}

pub enum Input {
    Repo(Target),
    Direct {
        url: String,
        hint_tag: Option<String>,
    },
}

fn strip_git(s: &str) -> &str {
    s.strip_suffix(".git").unwrap_or(s)
}

/// Acepta `https://github.com/o/r`, `github.com/o/r/releases/tag/v1`, `o/r`,
/// GitLab con subgrupos, Codeberg, y URLs directas a assets.
pub fn parse_input(raw: &str) -> Result<Input, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("URL vacía".into());
    }
    let s = s
        .split(['?', '#'])
        .next()
        .unwrap_or(s)
        .trim_end_matches('/');

    for (host, forge) in [
        ("github.com/", Forge::GitHub),
        ("gitlab.com/", Forge::GitLab),
        ("codeberg.org/", Forge::Codeberg),
    ] {
        let rest = match s.split_once(host) {
            Some((_, r)) => r,
            None => continue,
        };
        // URL directa a un asset de release: no resolvemos "último", usamos esa.
        for (marker, gitlab) in [("/releases/download/", false), ("/-/releases/", true)] {
            if let Some((_, after)) = rest.split_once(marker) {
                if gitlab && !after.contains("/downloads/") {
                    continue;
                }
                let tag = after.split('/').next().unwrap_or("").to_string();
                return Ok(Input::Direct {
                    url: s.to_string(),
                    hint_tag: (!tag.is_empty()).then_some(tag),
                });
            }
        }
        let (path, tag) = match rest.split_once("/releases/tag/") {
            Some((p, t)) => (p, Some(t.to_string())),
            None => (rest.split_once("/-/").map_or(rest, |(p, _)| p), None),
        };
        let path = path
            .split_once("/releases")
            .map_or(path, |(p, _)| p)
            .split_once("/tags")
            .map_or(path, |(p, _)| p);
        let repo = strip_git(path).trim_matches('/');
        if repo.split('/').count() < 2 {
            return Err(format!("no parece un repositorio: {raw}"));
        }
        return Ok(Input::Repo(Target {
            forge,
            repo: repo.to_string(),
            tag,
        }));
    }

    // Sin host conocido: asset directo.
    if s.contains("://") {
        let hint = version_in(s);
        return Ok(Input::Direct {
            url: s.to_string(),
            hint_tag: hint,
        });
    }
    // `owner/repo` a secas.
    if s.split('/').count() == 2 {
        return Ok(Input::Repo(Target {
            forge: Forge::GitHub,
            repo: s.to_string(),
            tag: None,
        }));
    }
    Err(format!("entrada no reconocida: {raw}"))
}

pub fn version_in(url: &str) -> Option<String> {
    let base = url.rsplit('/').next()?;
    let lower = base.to_ascii_lowercase();
    let ext = [
        ".tar.gz",
        ".tar.xz",
        ".tar.zst",
        ".tar.bz2",
        ".tgz",
        ".txz",
        ".appimage",
        ".zip",
        ".deb",
        ".rpm",
        ".gz",
        ".xz",
        ".zst",
        ".bz2",
    ]
    .iter()
    .find(|e| lower.ends_with(**e))?;
    let base = &base[..base.len() - ext.len()];
    for tok in
        base.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '+' || c == '~'))
    {
        let tok = tok.strip_prefix(['v', 'V']).unwrap_or(tok);
        if tok.len() >= 3
            && tok.starts_with(|c: char| c.is_ascii_digit())
            && tok.contains('.')
            && crate::version::valid(tok)
        {
            return Some(tok.to_string());
        }
    }
    None
}

/// Repo del que cuelga un asset directo de release, si se puede reconstruir.
pub fn target_from_asset(raw: &str) -> Option<Target> {
    let s = raw.trim();
    for (host, forge) in [
        ("github.com/", Forge::GitHub),
        ("gitlab.com/", Forge::GitLab),
        ("codeberg.org/", Forge::Codeberg),
    ] {
        let Some(rest) = s.split_once(host).map(|(_, r)| r) else {
            continue;
        };
        let repo = rest
            .split_once("/releases/")
            .or_else(|| rest.split_once("/-/releases/"))
            .map(|(p, _)| p)?;
        let mut parts = repo.split('/');
        let (owner, name) = (parts.next()?, parts.next()?);
        if owner.is_empty() || name.is_empty() {
            return None;
        }
        return Some(Target {
            forge,
            repo: format!("{owner}/{name}"),
            tag: None,
        });
    }
    None
}

/// Texto de la licencia del repo. Solo GitHub: es el 99% del índice y su API
/// devuelve el contenido en base64. GitLab/Codeberg se quedan con `--license`.
pub fn license_text(t: &Target, token: Option<&str>) -> Option<(String, String)> {
    if t.forge != Forge::GitHub {
        return None;
    }
    let mut c = Command::new("curl");
    c.args([
        "-fsSL",
        "--max-time",
        "20",
        "--retry",
        "2",
        "-H",
        "Accept: application/vnd.github+json",
    ]);
    if let Some(tok) = token {
        c.args(["-H", &format!("Authorization: {tok}")]);
    }
    c.arg(format!("https://api.github.com/repos/{}/license", t.repo));
    let body = run(c, None).ok()?;

    let mut j = Command::new("jq");
    j.args([
        "-r",
        r#"[.license.spdx_id // "", ((.content // "") | gsub("[^A-Za-z0-9+/=]"; ""))] | @tsv"#,
    ]);
    let line = run(j, Some(&body)).ok()?;
    let (spdx, encoded) = line.trim().split_once('\t')?;
    let text = String::from_utf8(b64decode(encoded)?).ok()?;
    (!text.trim().is_empty()).then(|| (spdx.to_string(), text))
}

fn b64decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut n) = (0u32, 0u8);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        n += 1;
        if n == 4 {
            out.push((acc >> 16) as u8);
            out.push((acc >> 8) as u8);
            out.push(acc as u8);
            (acc, n) = (0, 0);
        }
    }
    match n {
        0 => {}
        2 => out.push((acc >> 4) as u8),
        3 => {
            out.push((acc >> 10) as u8);
            out.push((acc >> 2) as u8);
        }
        _ => return None,
    }
    Some(out)
}

pub fn run(mut cmd: Command, input: Option<&str>) -> Result<String, String> {
    cmd.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "no se pudo ejecutar {}: {e}",
            cmd.get_program().to_string_lossy()
        )
    })?;
    if let Some(data) = input {
        use std::io::Write;
        child
            .stdin
            .take()
            .ok_or("stdin no disponible")?
            .write_all(data.as_bytes())
            .map_err(|e| format!("escribiendo a {}: {e}", cmd.get_program().to_string_lossy()))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("esperando a {}: {e}", cmd.get_program().to_string_lossy()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "{} falló ({}): {}",
            cmd.get_program().to_string_lossy(),
            out.status.code().unwrap_or(-1),
            err.trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| "salida no UTF-8".to_string())
}

fn curl(url: &str, token: Option<&str>, extra: &[(&str, &str)]) -> Result<String, String> {
    let mut c = Command::new("curl");
    c.args([
        "-fsSL",
        "--max-time",
        "40",
        "--retry",
        "3",
        "-H",
        "User-Agent: zardoid/0.1",
    ]);
    for (k, v) in extra {
        c.args(["-H", &format!("{k}: {v}")]);
    }
    if let Some(t) = token {
        c.args(["-H", &format!("Authorization: Bearer {t}")]);
    }
    c.arg(url);
    run(c, None)
}

fn jq(filter: &str, json: &str) -> Result<String, String> {
    let mut c = Command::new("jq");
    c.args(["-r", filter]);
    run(c, Some(json))
}

fn parse_rows(text: &str) -> Vec<(Release, Asset)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 5 {
            continue;
        }
        let size = f[4].parse().unwrap_or(0);
        out.push((
            Release {
                tag: f[0].to_string(),
                prerelease: f[1] == "true",
                assets: Vec::new(),
            },
            Asset {
                name: f[2].to_string(),
                url: f[3].to_string(),
                size,
            },
        ));
    }
    out
}

fn group(rows: Vec<(Release, Asset)>, tag: Option<&str>) -> Result<Release, String> {
    let mut releases: Vec<Release> = Vec::new();
    for (r, a) in rows {
        if let Some(want) = tag {
            if r.tag != want {
                continue;
            }
        }
        match releases.last_mut() {
            Some(last) if last.tag == r.tag => last.assets.push(a),
            _ => {
                let mut nr = r;
                nr.assets.push(a);
                releases.push(nr);
            }
        }
    }
    if releases.is_empty() {
        return Err(match tag {
            Some(t) => format!("el release {t} no tiene assets"),
            None => "el repositorio no tiene releases con assets".into(),
        });
    }
    // Los forges devuelven del más nuevo al más viejo.
    let stable = releases
        .iter()
        .position(|r| !r.prerelease && !crate::version::is_prerelease(&r.tag));
    Ok(match (tag, stable) {
        (Some(_), _) => releases.swap_remove(0),
        (None, Some(i)) => releases.swap_remove(i),
        (None, None) => releases.swap_remove(0),
    })
}

pub fn latest(t: &Target, token: Option<&str>) -> Result<Release, String> {
    let tag = t.tag.as_deref();
    let rows = match t.forge {
        Forge::GitHub => {
            let url = format!(
                "https://api.github.com/repos/{}/releases?per_page=30",
                t.repo
            );
            let body = curl(&url, token, &[("Accept", "application/vnd.github+json")])?;
            jq(
                r#".[] | select(.draft|not) | . as $r | ($r.assets[]? |
                   [$r.tag_name, ($r.prerelease|tostring), .name,
                    .browser_download_url, (.size|tostring)] | @tsv)"#,
                &body,
            )?
        }
        Forge::GitLab => {
            let enc = t.repo.replace('/', "%2F");
            let url = format!("https://gitlab.com/api/v4/projects/{enc}/releases?per_page=30");
            let mut c = Command::new("curl");
            c.args(["-fsSL", "--max-time", "40", "--retry", "3"]);
            if let Some(tk) = token {
                c.args(["-H", &format!("PRIVATE-TOKEN: {tk}")]);
            }
            c.arg(&url);
            let body = run(c, None)?;
            jq(
                r#".[] | select(.upcoming_release != true) | . as $r |
                   ($r.assets.links[]? | [$r.tag_name, "false", .name,
                    (.direct_asset_url // .url), "0"] | @tsv)"#,
                &body,
            )?
        }
        Forge::Codeberg => {
            let url = format!(
                "https://codeberg.org/api/v1/repos/{}/releases?limit=30",
                t.repo
            );
            let body = curl(&url, token, &[])?;
            jq(
                r#".[] | select(.draft|not) | . as $r | ($r.assets[]? |
                   [$r.tag_name, ($r.prerelease|tostring), .name,
                    .browser_download_url, (.size|tostring)] | @tsv)"#,
                &body,
            )?
        }
    };
    group(parse_rows(&rows), tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github() {
        match parse_input("https://github.com/nektos/act").unwrap() {
            Input::Repo(t) => {
                assert_eq!(t.repo, "nektos/act");
                assert_eq!(t.forge, Forge::GitHub);
                assert!(t.tag.is_none());
            }
            _ => panic!("repo"),
        }
        match parse_input("https://github.com/o/r/releases/tag/v1.2.3").unwrap() {
            Input::Repo(t) => assert_eq!(t.tag.as_deref(), Some("v1.2.3")),
            _ => panic!("tag"),
        }
    }

    #[test]
    fn parse_gitlab_subgroup() {
        match parse_input("https://gitlab.com/a/b/c/-/releases").unwrap() {
            Input::Repo(t) => {
                assert_eq!(t.repo, "a/b/c");
                assert_eq!(t.forge, Forge::GitLab);
            }
            _ => panic!("gitlab"),
        }
    }

    #[test]
    fn parse_direct_and_bare() {
        match parse_input("https://example.org/x/y-1.2.3.tar.gz").unwrap() {
            Input::Direct { hint_tag, .. } => assert_eq!(hint_tag.as_deref(), Some("1.2.3")),
            _ => panic!("direct"),
        }
        match parse_input("o/r").unwrap() {
            Input::Repo(t) => assert_eq!(t.repo, "o/r"),
            _ => panic!("bare"),
        }
        assert!(parse_input("nada").is_err());
    }

    #[test]
    fn rows_and_stable_pick() {
        let text = "v1.0\ttrue\ta.tar.gz\thttps://e/a\t10\n\
                    v0.9\tfalse\tb-linux-x86_64.tar.gz\thttps://e/b\t20\n\
                    v0.9\tfalse\tb-darwin.tar.gz\thttps://e/c\t30\n";
        let r = group(parse_rows(text), None).unwrap();
        assert_eq!(r.tag, "v0.9");
        assert_eq!(r.assets.len(), 2);
        let r = group(parse_rows(text), Some("v1.0")).unwrap();
        assert_eq!(r.tag, "v1.0");
        assert!(r.prerelease);
    }

    #[test]
    fn asset_target_and_b64() {
        let t = target_from_asset("https://github.com/o/r/releases/download/v1/x.tar.gz").unwrap();
        assert_eq!(t.repo, "o/r");
        assert_eq!(t.forge, Forge::GitHub);
        assert!(target_from_asset("https://example.org/a/b").is_none());
        assert_eq!(b64decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(b64decode("YQ==").unwrap(), b"a");
        assert_eq!(b64decode("YWI=").unwrap(), b"ab");
        assert!(b64decode("@@@").is_none());
    }

    #[test]
    fn version_hint() {
        assert_eq!(
            version_in("https://x/y/App-2.4.2-x86_64.AppImage").as_deref(),
            Some("2.4.2")
        );
        assert_eq!(version_in("https://x/y/app.tar.gz"), None);
        assert_eq!(
            version_in("https://x/y/app-v1.0.tar.gz").as_deref(),
            Some("1.0")
        );
    }
}
