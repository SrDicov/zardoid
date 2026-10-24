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

- [ ] **A1** `scripts/merge-validated.sh` acumula `meta/failed.list` con
  `cat >>`: un paquete reencolado que vuelve a fallar deja dos líneas. No hay
  dedupe. Impacto: la lista crece sin control y el triage miente.
- [ ] **A2** `versioned_url()` tiene una rama nueva (versión fecha de 8 dígitos
  permite `distfiles` sin `${version}`) sin test. Ruta tocada en 0.3.5, sin
  cobertura: un cambio futuro la rompe en silencio y el fallo aparece como
  404 en CI, 6 minutos después.
- [ ] **A3** Pipeline: 127 entradas reencoladas tras 0.3.7 sin verificar en CI.
  Terminar hasta cola 0 y triar los rechazos restantes.

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
