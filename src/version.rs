//! Normalización de tags upstream -> campo `version=` de XBPS.

/// Caracteres que acepta el detector del VUR (`detect-updates.py:113`):
/// `^[0-9][0-9A-Za-z.+~]*$`. `-` y `_` están prohibidos (`_` separa revisión).
pub fn valid(v: &str) -> bool {
    let mut c = v.chars();
    match c.next() {
        Some(f) if f.is_ascii_digit() => {}
        _ => return false,
    }
    c.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '~'))
}

const PRE: [&str; 6] = ["alpha", "beta", "pre", "rc", "dev", "nightly"];

pub fn is_prerelease(v: &str) -> bool {
    let b = v.as_bytes();
    let lower = v.to_ascii_lowercase();
    for p in PRE {
        let mut from = 0;
        while let Some(i) = lower[from..].find(p) {
            let i = from + i;
            let before_ok = i == 0 || !b[i - 1].is_ascii_alphabetic();
            let j = i + p.len();
            let after_ok = j == b.len() || !b[j].is_ascii_alphabetic();
            if before_ok && after_ok {
                return true;
            }
            from = i + 1;
        }
    }
    false
}

/// `v1.2.3` -> `1.2.3`; `release-1.2.3` -> `1.2.3`; `1.2.3-rc1` -> `1.2.3rc1`.
/// Devuelve `None` si el resultado no es una versión válida para XBPS.
pub fn from_tag(tag: &str) -> Option<String> {
    let mut t = tag.trim().trim_matches('"').trim();
    // Tags con path (`deploy/v1.2.3-4`): manda el último segmento.
    if let Some((_, last)) = t.rsplit_once('/') {
        t = last;
    }
    for p in ["release-", "release/", "version-", "tag-"] {
        if let Some(r) = t.strip_prefix(p) {
            t = r;
            break;
        }
    }
    if let Some(r) = t.strip_prefix(['v', 'V']) {
        if r.starts_with(|c: char| c.is_ascii_digit()) {
            t = r;
        }
    }
    // `1.2.3-4` se queda como 1.2.3.4 (convención existente); no se confunde
    // con `deploy/v10.2.3-68`, que llega ya recortado al último segmento.

    // `foo-1.2.3` / `foo-v1.2.3`: prefijo con guion y sufijo numérico.
    if let Some((l, r)) = t.rsplit_once('-') {
        let r = r.strip_prefix(['v', 'V']).unwrap_or(r);
        if l.chars().any(|c| c.is_ascii_alphabetic()) && r.starts_with(|c: char| c.is_ascii_digit())
        {
            t = r;
        }
    }
    // Separadores prohibidos. `-rc1` -> `rc1` (convención Void), resto -> `.`.
    let mut out = String::with_capacity(t.len());
    let chars: Vec<char> = t.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' && i + 1 < chars.len() {
            let rest: String = chars[i + 1..].iter().take(8).collect();
            let lower = rest.to_ascii_lowercase();
            if PRE.iter().any(|p| lower.starts_with(p)) {
                continue;
            }
        }
        out.push(match c {
            '-' | '_' => '.',
            c => c,
        });
    }
    let out = out.trim_matches('.').to_string();
    valid(&out).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        assert_eq!(from_tag("v1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(from_tag("V2.0.1").as_deref(), Some("2.0.1"));
        assert_eq!(from_tag("release-1.4.0").as_deref(), Some("1.4.0"));
        assert_eq!(from_tag("foo-bar-3.1").as_deref(), Some("3.1"));
        assert_eq!(from_tag("1.2.3-rc1").as_deref(), Some("1.2.3rc1"));
        assert_eq!(from_tag("1.2.3-beta.2").as_deref(), Some("1.2.3beta.2"));
        assert_eq!(from_tag("2026.07+3").as_deref(), Some("2026.07+3"));
        assert_eq!(from_tag("v1.0.0_1").as_deref(), Some("1.0.0.1"));
        assert_eq!(from_tag("proj-v1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(from_tag("1.2.3-1").as_deref(), Some("1.2.3.1"));
        assert_eq!(from_tag("deploy/v10.2.3-68").as_deref(), Some("10.2.3.68"));
        assert_eq!(from_tag("release/v1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(from_tag("nightly"), None);
        assert_eq!(from_tag("nope!"), None);
        assert_eq!(from_tag("config"), None);
    }

    #[test]
    fn charset() {
        assert!(valid("1.0"));
        assert!(valid("1.58.0+a"));
        assert!(!valid("v1.0"));
        assert!(!valid("1.0-1"));
        assert!(!valid("1.0_1"));
        assert!(!valid(""));
    }

    #[test]
    fn prerelease() {
        assert!(is_prerelease("1.0rc1"));
        assert!(is_prerelease("2.0-beta.1"));
        assert!(!is_prerelease("1.0.0a"));
        assert!(!is_prerelease("2026.07+3"));
        assert!(!is_prerelease("1.58.0+a"));
    }
}
