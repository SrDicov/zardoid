#!/bin/sh
# Vuelca meta/belka.tsv (índice de Belka) a meta/requests.list.
# Solo importa una vez cada URL: el registro está en meta/belka.imported.
#
# Uso: sh scripts/import-belka.sh [meta/belka.tsv]

set -eu

TSV=${1:-meta/belka.tsv}
REQ=meta/requests.list
SEEN=meta/belka.imported

[ -f "$TSV" ] || exit 0
[ -f "$REQ" ] || : >"$REQ"
[ -f "$SEEN" ] || : >"$SEEN"

while IFS="$(printf '\t')" read -r name _version url _sha _desc _exec _cat _term tipo; do
	case $tipo in
	tar | appimage) ;;
	*) continue ;;
	esac
	[ -n "$url" ] || continue
	grep -qxF -- "$url" "$SEEN" && continue
	printf '%s %s-bin\n' "$url" "$name" >>"$REQ"
	printf '%s\n' "$url" >>"$SEEN"
done <"$TSV"
