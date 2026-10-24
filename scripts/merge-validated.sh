#!/bin/sh
# Copia a srcpkgs/ los templates que pasaron xlint Y compilaron, saca de la
# cola todo el lote procesado y anota los rechazos en meta/failed.list.
#
# Uso: sh scripts/merge-validated.sh <staging> <validated> <built>

set -eu

STAGING=${1:?falta el directorio de staging}
VALIDATED=${2:?falta la lista de validados}
BUILT=${3:?falta la lista de compilados}
[ -f "$VALIDATED" ] || : >"$VALIDATED"
[ -f "$BUILT" ] || : >"$BUILT"
[ -f "$STAGING/lote.tsv" ] || : >"$STAGING/lote.tsv"
[ -f "$STAGING/failed_gen.tsv" ] || : >"$STAGING/failed_gen.tsv"

ok=$(mktemp)
sort -u "$VALIDATED" >"$ok.v"
sort -u "$BUILT" >"$ok.b"
comm -12 "$ok.v" "$ok.b" >"$ok"

merged=0
while IFS= read -r pkg; do
	case $pkg in ''|'#'*) continue ;; esac
	case $pkg in *[!a-z0-9+._-]*) echo "paquete inválido: $pkg" >&2; continue ;; esac
	src="$STAGING/srcpkgs/$pkg/template"
	[ -f "$src" ] || continue
	mkdir -p "srcpkgs/$pkg"
	cp "$src" "srcpkgs/$pkg/template"
	if [ -d "$STAGING/srcpkgs/$pkg/files" ]; then
		mkdir -p "srcpkgs/$pkg/files"
		cp -R "$STAGING/srcpkgs/$pkg/files/." "srcpkgs/$pkg/files/"
	fi
	printf 'merge %s\n' "$pkg"
	merged=$((merged + 1))
done <"$ok"

# La cola solo avanza: todo el lote sale, se haya mergeado o no.
procesadas=$(mktemp)
cut -f1 "$STAGING/lote.tsv" >>"$procesadas"
cut -f1 "$STAGING/failed_gen.tsv" >>"$procesadas"
if [ -s "$procesadas" ] && [ -f meta/requests.list ]; then
	awk -v drop="$procesadas" '
		BEGIN { while ((getline u < drop) > 0) d[u] = 1 }
		{ if (!($1 in d)) print }
	' meta/requests.list >"$procesadas.q"
	mv "$procesadas.q" meta/requests.list
fi

# Rechazos: generación y lote que no pasó xlint/build.
fallos=$(mktemp)
cat "$STAGING/failed_gen.tsv" >>"$fallos" 2>/dev/null || true
while IFS="$(printf '\t')" read -r url pkg; do
	[ -n "$url" ] || continue
	[ -n "$pkg" ] || continue
	grep -qxF -- "$pkg" "$ok" && continue
	printf '%s\t%s\t%s\n' "$url" "$pkg" "xlint/build" >>"$fallos"
done <"$STAGING/lote.tsv"
if [ -s "$fallos" ]; then
	cat "$fallos" >>meta/failed.list
	# Un paquete reencolado que vuelve a fallar no debe dejar dos líneas.
	awk -F"$(printf '\t')" '!/^#/ && seen[$1]++ { next } { print }' \
		meta/failed.list >"$fallos"
	mv "$fallos" meta/failed.list
fi

echo "templates copiados: $merged"
rm -f "$ok" "$ok.v" "$ok.b" "$procesadas" "$fallos"
