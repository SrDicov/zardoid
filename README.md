# Zardoid

Genera un `template` de Void Linux (`-bin`) desde la URL de un release upstream.
Analiza el asset, lo descarga, lo inspecciona y emite el template ya formateado.

Rust, **cero crates**. En runtime usa `curl` (HTTP), `jq` (JSON), `tar`/`unzip`
(archivos) y `sha256sum` (hash).

## Instalación

```sh
curl -sLO https://github.com/SrDicov/zardoid/releases/latest/download/zardoid-x86_64-linux.tar.gz
tar -xzf zardoid-x86_64-linux.tar.gz -C ~/.local/bin
```

O desde el código: `cargo build --release` (`target/release/zardoid`).

## Uso

```sh
cargo build --release

# repo: resuelve el último release estable y elige el asset Linux x86_64
./target/release/zardoid https://github.com/BurntSushi/ripgrep \
    --maintainer "Tu Nombre <tu@correo.org>"

# escribe srcpkgs/<pkg>/template en vez de stdout
./target/release/zardoid https://codeberg.org/usuario/repo \
    --maintainer "Tu Nombre <tu@correo.org>" --out ~/z-packages

# asset concreto (se congela esa URL)
./target/release/zardoid https://github.com/o/r/releases/download/v1.2.3/app-linux-x86_64.tar.gz \
    --maintainer "Tu Nombre <tu@correo.org>"
```

Opciones: `--name`, `--desc`, `--license`, `--maintainer`, `--out`, `--json`,
`--force`, `--token`. `--maintainer` es obligatorio: `xlint` no acepta
placeholders. `--json` requiere `--out`.

Variables de entorno:

- `GITHUB_TOKEN`, `GITLAB_TOKEN`, `CODEBERG_TOKEN`: token de API de cada forge
  (nunca se envía el de un forge a otro). `--token` vale para el forge de la URL.
- `ZARDOID_MAINTAINER`: valor por defecto de `--maintainer`.
- `ZARDOID_FORMATTER`: ruta a `format-template.py`; si existe, se ejecuta
  `--check` sobre el template antes de escribirlo.

## Qué hace

1. Parsea la URL: GitHub, GitLab (subgrupos), Codeberg, asset directo o `owner/repo`.
2. Consulta los releases, descarta drafts/prereleases y elige el asset Linux
   x86_64 (tarball, zip, AppImage, binario comprimido o ejecutable suelto)
   por puntuación. Acepta `x86_64`, `amd64`, `x64` y `x86-64`, y descarta
   tarballs de fuentes cuando hay un binario disponible.
3. Normaliza el tag a `version=` (`v1.2.3` → `1.2.3`, `1.2.3-rc1` → `1.2.3rc1`).
4. Descarga, calcula `sha256` y extrae en un directorio temporal.
5. Inspecciona: ELF real (máquina, glibc/musl/estático → `archs`), binarios,
   `.desktop`, iconos, man, completions y licencias.
6. Emite el template en orden canónico, ≤80 columnas, sin comentarios tras la
   línea 1, con `distfiles` derivable de `${version}`.
7. Verifica con `bash -n` y (opcionalmente) `format-template.py --check`.

Códigos de salida: `0` ok · `1` uso/validación · `2` red/API · `3` verificación
(`bash -n`, formatter, versión no derivable) · `4` escritura.

## Límites (v1)

- Solo paquetes precompilados (`-bin`). No genera paquetes desde fuente.
- No soporta `.deb` ni `.rpm`.
- **Nunca ejecuta el binario descargado.** Para AppImage, la extracción
  (`--appimage-extract`) ocurre en tiempo de build dentro de `xbps-src`.
- `maintainer` y, si el forge no la detecta, `license` son decisiones humanas.
- `untracked`: los assets con `/latest/` en la URL se congelan a los bytes
  actuales y se avisa por stderr.
- Las líneas de más de 80 columnas (rutas de iconos largas, URLs) se avisan por
  stderr pero **no** bloquean: `xlint` no impone ese límite, solo lo recomienda
  el Manual.
