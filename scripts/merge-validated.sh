#!/bin/sh
# Copia a srcpkgs/ los templates validados y borra de la cola sus URLs.
#
# Uso: sh scripts/merge-validated.sh <staging> <lista-de-paquetes>

set -eu

STAGING=${1:?falta el directorio de staging}
VALIDATED=${2:?falta la lista de paquetes validados}
[ -f "$VALIDATED" ] || exit 0

mkdir -p srcpkgs
merged=0
while IFS= read -r pkg; do
	case $pkg in ''|'#'*) continue ;; esac
	case $pkg in *[!a-z0-9+._-]*) echo "paquete inválido: $pkg" >&2; continue ;; esac
	src="$STAGING/srcpkgs/$pkg/template"
	[ -f "$src" ] || continue
	mkdir -p "srcpkgs/$pkg"
	cp "$src" "srcpkgs/$pkg/template"
	printf 'merge %s\n' "$pkg"
	merged=$((merged + 1))
done <"$VALIDATED"
echo "templates copiados: $merged"

if [ "$merged" -eq 0 ] || [ ! -f meta/requests.list ] || [ ! -f "$STAGING/map.tsv" ]; then
	exit 0
fi

# URLs ya mergeadas: las que mapean a un paquete de la lista validada.
tmp=$(mktemp)
while IFS="$(printf '\t')" read -r url pkg; do
	if grep -qxF "$pkg" "$VALIDATED"; then printf '%s\n' "$url"; fi
done <"$STAGING/map.tsv" >"$tmp.urls"

if [ -s "$tmp.urls" ]; then
	awk -v dropfile="$tmp.urls" '
		BEGIN { while ((getline u < dropfile) > 0) d[u] = 1 }
		{ if (!($1 in d)) print }
	' meta/requests.list >"$tmp"
	mv "$tmp" meta/requests.list
else
	rm -f "$tmp"
fi
rm -f "$tmp.urls"
