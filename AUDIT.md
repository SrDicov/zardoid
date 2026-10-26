# AUDIT — Zardoid (overnight 2026-10-24)

Contrato de la noche. Un ítem por línea, marcado al completarse.
Base: `dfdd8d4` (0.3.7). Rama: `agent/overnight-20261024-0045`.

## Estado de partida (verificado, no supuesto)

- `cargo build --release`: OK.
- `cargo clippy --all-targets -- -D warnings`: OK, 0 warnings.
- `cargo test`: 32 + 5 passed, 0 failed.
- `grep TODO|FIXME|XXX`: 0.
- `unwrap()/expect()/panic!` fuera de tests: 0.
- `git status`: limpio.

## Defectos encontrados

- [x] **A1** `scripts/merge-validated.sh` acumula `meta/failed.list` con
  `cat >>`: un paquete reencolado que vuelve a fallar deja dos líneas. No hay
  dedupe. Impacto: la lista crece sin control y el triage miente.
- [x] **A2** `versioned_url()` tiene una rama nueva (versión fecha de 8 dígitos
  permite `distfiles` sin `${version}`) sin test. Ruta tocada en 0.3.5, sin
  cobertura: un cambio futuro la rompe en silencio y el fallo aparece como
  404 en CI, 6 minutos después.
- [x] **A3** Pipeline: 127 entradas reencoladas tras 0.3.7. Cerrado con cola 0,
  558 templates y 98 rechazos triados (64 generación, 31 xlint/build).
  Bug nuevo encontrado y corregido: `vcopy share/man/*` sin `vmkdir` previo
  mataba el build con ENOENT (gh-bin, genovalauncher-bin, intermodal-bin,
  jetbrains-toolbox-bin).

## Triage de los 31 rechazos de build (no reencolados)

- 8 × `file ... has write permission for other users` (bigpemu, clonehero,
  gcap2025/2026, irpf2025/2026, itr2025, srb2): el zip upstream trae modos
  0666. Fix pendiente: `chmod -R o-w` tras el `vcopy` del árbol.
- 1 × `zoxide-bin`: ya arreglado por el `vmkdir` de 0.3.8, pendiente reencolar.
- 1 × `broot-bin`: el zip multi-arch contiene `aarch64-.../broot` y el
  generador lo elige; falta preferir x86_64 en `main_bin`.
- 1 × `appimagetool-bin`: el índice apunta a un tarball `corresponding-source`,
  no a un binario. El generador debería rechazarlo en vez de empaquetar un
  ELF aarch64 de test.
- 1 × `galculator-bin`: el asset del índice es `anylinux-loongarch64` y
  `foreign_platform` no reconoce `loongarch64`.
- 1 × `xournalpp-bin`: tar con `.so` propias, `cannot guess required shlibs`.
  Falta `allow_unknown_shlibs=yes` en los genéricos.
- 11 sin log disponible (runs caducados): reencolar y ver.

## Deuda aceptada (no se toca esta noche)

- `short_desc` de los AppImage de pkgforge arrastra `[Maintainer=@user]` del
  upstream. Cosmético, válido para xlint. Arreglarlo exige reescribir ~300
  templates ya mergeados: refactor colateral, fuera de alcance.
- `python_version=3` y `allow_unknown_shlibs=yes` se emiten en todos los
  AppImage, no solo en los que los necesitan. Una línea de ruido por template
  a cambio de no inspeccionar 100 MB por paquete. Aceptado.
- `schedule` de GitHub muerto (0 runs en 6 h). Sustituido por encadenado con
  `workflow_dispatch` desde el job de merge.

## Fuera de alcance

- Los 40 rechazos de generación por plataforma ajena (FreeBSD/macOS/Windows):
  el índice de Belka apunta a assets que no existen para Void x86_64.

## Cierre 0.3.12 (indice vivo, cola 0)

Se refresco `meta/belka.tsv` desde el indice publicado de Belka (643 lineas,
333 URLs nuevas) y se proceso la cola completa. Estado final:

- 595 templates en `srcpkgs/` para 618 paquetes distintos del indice.
- 0 entradas en `meta/requests.list`.
- 54 rechazos, todos de URLs que siguen en el indice vivo (los 16 obsoletos
  se podaron): 45 por plataforma ajena (FreeBSD/DragonFly/macOS/LoongArch/ARM)
  y 9 irrecuperables para este pipeline:
  - 3 archivos de fuentes de GitHub (`/archive/`): mfetch, pfetch, ranger.
  - 3 tags moviles sin version (`rolling`/`latest`): epubworm, trinity, tui-timer.
  - 1 release con solo firmas `.asc` (helm; los binarios viven en get.helm.sh).
  - 1 URL muerta (tor-browser 404).
  - 1 bootloader sin ELF Linux (limine: solo blobs EFI/BIOS).

### Arreglos nuevos (0.3.10 - 0.3.12)

- `vman` por pagina: se elimino el `vcopy share/man/*`, que fallaba con
  `cp: cannot stat` cuando el tarball no tenia `share/man/`.
- Licencias: SPDX del `LICENSE` extraido (respetando `inner_root`), tabla de
  overrides verificados (MPL/GPL), repo upstream para AppImage de pkgforge y
  aviso `custom:*` para propietario/desconocido.
- `inventory`: no confunde `libXss.so.1` con un manual ni `license.jar` con
  una licencia.
- Fallback al release del repo cuando el asset indexado es fuente, firma,
  `.deb` o no tiene version (feroxbuster, appimagetool).
- Tags con `/` (`deploy/v10.2.3-68`) y URLs literales cuando la version sale
  del tag (osu, binfmt).
- Scripts con shebang: Belka y ramfetch se empaquetan con `vbin` y sin
  `archs`; `belka-bin` declara `depends="curl jq"`.
- Contenedor de build con `zstd` para `.pkg.tar.zst` (steplauncher).

### Deuda remanente

- 54 assets del indice no son empaquetables para Void x86_64 (plataforma
  ajena, fuentes, tags moviles o URLs muertas). Reintentarlos quema runner.
- `short_desc` de los AppImage de pkgforge conserva `[Maintainer=@user]`.
