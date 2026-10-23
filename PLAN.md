# Zardoid — plan de construcción

Generador de templates de Void Linux a partir de una URL de release.
Rust, **cero crates**. HTTP con `curl`, JSON con `jq`, hashes con `sha256sum`,
archivos con `tar`/`unzip`. Todos son requisitos del ecosistema Void/VUR.

## Contrato

```
zardoid <url> [--name P] [--maintainer "N <m>"] [--out DIR] [--json] [--force]
```

- Entrada: URL de repo (github/gitlab/codeberg) o URL directa de asset.
- Salida: `DIR/srcpkgs/<pkg>/template` (o stdout sin `--out`).
- El template emitido debe pasar `format-template.py --check` y `bash -n` a la primera.

## Fases y responsables

| Fase | Trabajo | Quién | Gate |
|---|---|---|---|
| F0 | Reconocimiento local + investigación web | 3 subagentes (APIs, xlint, versiones) | informes |
| F1 | Núcleo puro: `version`, `asset`, `elf`, `template` | main | ✅ `cargo test` |
| F2 | `forge`: parseo de URL, curl+jq por forge, modelo `Release` | main | ✅ 3 forges en vivo |
| F3 | `inspect`: descarga, sha256, listado/extracción, inventario | main | ✅ tests |
| F4 | `main`: CLI, pipeline, autochequeo | main | ✅ `cargo build` |
| F5 | Tests deterministas (golden + unitarios) | main | ✅ `cargo test` |
| F6 | Validación real contra 7 casos (GitHub×3, Codeberg, GitLab, AppImage, zip) | main | ✅ 7/7 `fmt`+`bash -n` |
| F7 | Auditoría de corrección (diff completo) | subagente reviewer | ✅ 15 hallazgos |
| F8 | Correcciones + re-test + re-validación | main | ✅ 29 tests, clippy limpio |
| F9 | Aceptación: idempotencia, `bash -n`, README | main + subagente | ✅ verde |

## Decisiones de diseño

- **Datos**: `Release{tag, assets}` → `Asset{name,url,size}` → `Inventory{binaries,desktop,icons,man,licenses,interp}` → `Spec` → texto. Sin estado global.
- **Puntuación de assets**: tabla de rechazo (otras OS/arch, firmas, checksums) y preferencia (linux+x86_64+gnu, AppImage > tarball > deb/rpm > binario suelto). Función pura, testeable.
- **ELF sin `file`**: cabecera + `PT_INTERP` leídos a mano → arq, glibc/musl, estático, PIE. Es lo que decide `archs` y `nostrip`.
- **Emisión**: orden canónico de `format-template.py` (`ORDER`/`AUX_INDEX`), ≤80 columnas con continuación de un espacio, cero comentarios tras la línea 1.
- **Sin `local`/bashismos en templates**: son bash de xbps, pero deben ser legibles; cuerpo con tabs.
- **Techo conocido**: solo releases con assets precompilados (v1). Fuente propia → fuera de alcance, se declara en README.

## Subagentes

1. `research-apis` — GitHub/GitLab/Codeberg: campos, rate limit, edge cases.
2. `research-xlint` — reglas de `xlint` y convenciones `-bin`/AppImage.
3. `research-version` — charset real de `version`, normalización de tags, CalVer.
4. `validate-live` — corre Zardoid contra repos reales; compara con templates existentes de z-packages; `format-template.py --check` + `bash -n`. Solo lectura del repo, escribe informe.
5. `reviewer` — auditoría del diff completo: bugs, casos no cubiertos, salida no idempotente.
6. `adversarial` — propone entradas que rompen el parser (tags raros, assets ausentes, URLs no-repo); verifica que fallan con código de salida correcto.
