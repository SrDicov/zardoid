//! Emisor del `template`. Orden canónico de `format-template.py`, 80 columnas,
//! cero comentarios tras la línea 1.

pub struct Spec {
    pub pkgname: String,
    pub version: String,
    pub revision: u32,
    pub fields: Vec<(String, String)>,
    pub functions: Vec<(String, String)>,
}

/// Rango canónico. Mismo criterio que `ORDER`/`AUX_INDEX`/`UNKNOWN_INDEX` de
/// `.github/scripts/format-template.py` del VUR.
fn rank(key: &str) -> (u16, String) {
    let known = match key {
        "pkgname" => 10,
        "reverts" => 20,
        "version" => 30,
        "revision" => 40,
        "archs" => 50,
        "create_wrksrc" => 80,
        "build_wrksrc" => 90,
        "build_style" => 100,
        "build_helper" => 105,
        "metapackage" => 110,
        "hostmakedepends" => 150,
        "makedepends" => 160,
        "depends" => 170,
        "checkdepends" => 180,
        "short_desc" => 190,
        "maintainer" => 200,
        "license" => 210,
        "homepage" => 220,
        "changelog" => 230,
        "distfiles" => 240,
        "checksum" => 250,
        "alternatives" => 260,
        "fetch_cmd" => 265,
        _ => 0,
    };
    if known != 0 {
        return (known, key.to_string());
    }
    if key.starts_with('_') {
        return (235, key.to_string()); // auxiliares: justo antes de distfiles
    }
    const GROUP12: &[&str] = &[
        "cmake_args",
        "configure_args",
        "configure_script",
        "go_build_tags",
        "go_import_path",
        "go_ldflags",
        "go_package",
        "go_mod_mode",
        "make_build_args",
        "make_build_target",
        "make_check_args",
        "make_check_target",
        "make_cmd",
        "make_install_args",
        "make_install_target",
        "make_use_env",
        "meson_args",
        "python_versions",
    ];
    if GROUP12.contains(&key) {
        return (120, key.to_string());
    }
    (270, key.to_string())
}

fn quoted(v: &str) -> bool {
    v.len() >= 2 && v.starts_with('"') && v.ends_with('"')
}

/// Parte una línea larga en continuaciones de **un solo espacio**. Una URL
/// nunca se parte: bash concatenaría con un espacio y la rompería.
fn emit_field(key: &str, val: &str, out: &mut String) {
    let line = format!("{key}={val}");
    if line.len() <= 80 || !quoted(val) || val.contains("://") {
        out.push_str(&line);
        out.push('\n');
        return;
    }
    let inner = &val[1..val.len() - 1];
    let words: Vec<&str> = inner.split_whitespace().collect();
    if words.len() < 2 {
        out.push_str(&line);
        out.push('\n');
        return;
    }
    let mut cur = format!("{key}=\"");
    let mut first = true;
    for w in words {
        let extra = if first { 0 } else { 1 };
        if !first && cur.len() + extra + w.len() + 1 > 80 {
            out.push_str(&cur);
            out.push('\n');
            cur = " ".to_string();
            first = true;
        }
        if !first {
            cur.push(' ');
        }
        cur.push_str(w);
        first = false;
    }
    cur.push('"');
    out.push_str(&cur);
    out.push('\n');
}

pub fn render(spec: &Spec) -> String {
    let mut fields = spec.fields.clone();
    fields.push(("pkgname".into(), spec.pkgname.clone()));
    fields.push(("version".into(), spec.version.clone()));
    fields.push(("revision".into(), spec.revision.to_string()));
    fields.sort_by_key(|(k, _)| rank(k));

    let mut out = format!("# Template file for '{}'\n", spec.pkgname);
    for (k, v) in &fields {
        emit_field(k, v, &mut out);
    }

    const FN_ORDER: &[&str] = &[
        "do_extract",
        "post_extract",
        "pre_configure",
        "do_configure",
        "post_configure",
        "pre_build",
        "do_build",
        "post_build",
        "pre_check",
        "do_check",
        "post_check",
        "pre_install",
        "do_install",
        "post_install",
    ];
    let mut fns = spec.functions.clone();
    fns.sort_by_key(|(n, _)| {
        (
            FN_ORDER
                .iter()
                .position(|f| f == n)
                .unwrap_or(FN_ORDER.len()),
            n.clone(),
        )
    });
    for (name, body) in &fns {
        out.push('\n');
        out.push_str(name);
        out.push_str("() {\n");
        for line in body.lines() {
            if line.trim().is_empty() {
                continue;
            }
            out.push('\t');
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out.push_str("}\n");
    }
    out
}

/// Comprobaciones que `format-template.py --check` también hace y que aquí
/// detectamos antes de escribir: header, comentarios y longitud.
pub fn selfcheck(text: &str) -> Vec<String> {
    let mut errs = Vec::new();
    let mut lines = text.lines();
    match lines.next() {
        Some(l) if l.starts_with("# Template file for '") && l.ends_with('\'') => {}
        _ => errs.push("línea 1 debe ser \"# Template file for '<pkgname>'\"".into()),
    }
    for (i, l) in text.lines().enumerate().skip(1) {
        if l.trim_start().starts_with('#') {
            errs.push(format!("comentario # en línea {}", i + 1));
        }
        // `short_desc` y `distfiles` no se parten nunca (ni xlint ni el
        // formateador lo hacen), así que no cuentan para las 80 columnas.
        if l.len() > 80 && !l.contains("://") && !l.starts_with("short_desc=") {
            errs.push(format!("línea {} supera 80 columnas", i + 1));
        }
        if l.contains(" \t") || l.starts_with(' ') && l.len() > 1 && !l[1..].starts_with(' ') {
            // continuación válida = exactamente un espacio inicial
            if l.starts_with(' ') && !l.starts_with("  ") && !l.starts_with(" \t") {
                continue;
            }
            errs.push(format!("indentación no canónica en línea {}", i + 1));
        }
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Spec {
        Spec {
            pkgname: "demo-bin".into(),
            version: "1.2.3".into(),
            revision: 1,
            fields: vec![
                ("archs".into(), "\"x86_64\"".into()),
                (
                    "short_desc".into(),
                    "\"Demo tool for testing zardoid output\"".into(),
                ),
                ("maintainer".into(), "\"T <t@example.org>\"".into()),
                ("license".into(), "\"MIT\"".into()),
                (
                    "distfiles".into(),
                    "\"https://e.org/v${version}/demo-${version}.tar.gz\"".into(),
                ),
                ("checksum".into(), "a".repeat(64)),
                ("nostrip".into(), "yes".into()),
            ],
            functions: vec![("do_install".into(), "vbin demo".into())],
        }
    }

    #[test]
    fn canonical_first_lines() {
        let t = render(&sample());
        let l: Vec<&str> = t.lines().collect();
        assert_eq!(l[0], "# Template file for 'demo-bin'");
        assert_eq!(l[1], "pkgname=demo-bin");
        assert_eq!(l[2], "version=1.2.3");
        assert_eq!(l[3], "revision=1");
        assert!(t.contains("nostrip=yes"));
        assert!(t.ends_with("}\n"));
        assert!(selfcheck(&t).is_empty(), "{:?}", selfcheck(&t));
    }

    #[test]
    fn long_list_wraps_with_one_space() {
        let mut s = sample();
        s.fields.push((
            "depends".into(),
            "\"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi\"".into(),
        ));
        let t = render(&s);
        let cont = t
            .lines()
            .find(|l| l.starts_with(' ') && !l.starts_with("  "))
            .expect("continuación");
        assert!(cont.starts_with(' ') && !cont.starts_with("  "));
        assert!(selfcheck(&t).is_empty(), "{:?}", selfcheck(&t));
    }

    #[test]
    fn long_url_never_split() {
        let mut s = sample();
        let url = format!("\"https://example.org/{}/file.tar.gz\"", "x".repeat(90));
        s.fields.push(("distfiles".into(), url.clone()));
        let t = render(&s);
        assert!(t.contains(&url));
    }

    #[test]
    fn comments_are_flagged() {
        let t = "# Template file for 'a'\npkgname=a\n# nota\n";
        assert_eq!(selfcheck(t).len(), 1);
    }

    #[test]
    fn aux_before_distfiles() {
        let mut s = sample();
        s.fields.push(("_asset".into(), "\"a.tar.gz\"".into()));
        let t = render(&s);
        let aux = t.find("_asset=").unwrap();
        let dist = t.find("distfiles=").unwrap();
        let home = t.find("homepage=");
        assert!(aux < dist);
        assert!(home.is_none() || aux > home.unwrap());
    }
}
