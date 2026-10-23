#!/bin/sh
# Genera un template por entrada de meta/requests.list en un directorio de
# staging. Procesa como mucho LOTE entradas (0 = todas) y no aborta: anota
# los fallos y sigue con la siguiente.
#
# Uso: [LOTE=20] sh scripts/gen-queue.sh [staging]

set -eu

OUT=${1:-staging}
BIN=${ZARDOID_BIN:-./target/release/zardoid}
REQ=meta/requests.list
MAINT_FILE=meta/maintainer
LOTE=${LOTE:-0}

[ -x "$BIN" ] || { echo "falta el binario $BIN (cargo build --release)" >&2; exit 2; }
[ -f "$REQ" ] || { echo "falta $REQ" >&2; exit 2; }
[ -f "$MAINT_FILE" ] || { echo "falta $MAINT_FILE" >&2; exit 2; }

MAINT=$(sed -n '1p' "$MAINT_FILE")
mkdir -p "$OUT/logs"
: >"$OUT/map.tsv"
: >"$OUT/lote.tsv"
: >"$OUT/failed_gen.tsv"
: >"$OUT/failed.txt"
export XDG_CACHE_HOME="$OUT/.cache"
n=0
ok=0

while IFS= read -r line; do
	case $line in ''|'#'*) continue ;; esac
	if [ "$LOTE" -gt 0 ] && [ "$n" -ge "$LOTE" ]; then
		break
	fi
	url=$(printf '%s\n' "$line" | awk '{print $1}')
	name=$(printf '%s\n' "$line" | awk 'NF>1{print $2}')
	lic=$(printf '%s\n' "$line" | awk 'NF>2{print $3}')
	[ -n "$url" ] || continue
	n=$((n + 1))

	set -- "$url" --maintainer "$MAINT" --out "$OUT" --json --force
	if [ -n "$name" ]; then set -- "$@" --name "$name"; fi
	if [ -n "$lic" ]; then set -- "$@" --license "$lic"; fi

	if json=$("$BIN" "$@" 2>"$OUT/logs/$n.log"); then
		pkg=$(printf '%s\n' "$json" | jq -r '.pkgname // empty')
		if [ -n "$pkg" ]; then
			printf '%s\t%s\n' "$url" "$pkg" >>"$OUT/map.tsv"
			printf '%s\t%s\n' "$url" "$pkg" >>"$OUT/lote.tsv"
			printf 'ok    %s -> %s\n' "$url" "$pkg"
			ok=$((ok + 1))
			rm -rf "$OUT/.cache/zardoid"
			continue
		fi
	fi
	printf '%s\t%s\t%s\n' "$url" "${name:--}" "generacion" >>"$OUT/failed_gen.tsv"
	printf '%s\n' "$url" >>"$OUT/failed.txt"
	printf 'FALLO %s (log: %s/logs/%s.log)\n' "$url" "$OUT" "$n" >&2
	rm -rf "$OUT/.cache/zardoid"
done <"$REQ"

{
	printf '## Templates generados\n\n'
	printf -- '- entradas: %s\n- generados: %s\n- fallidos: %s\n\n' \
		"$n" "$ok" "$(wc -l <"$OUT/failed.txt" | tr -d ' ')"
	if [ -s "$OUT/lote.tsv" ]; then
		printf '| paquete | url |\n|---|---|\n'
		while IFS="$(printf '\t')" read -r u p; do
			printf '| `%s` | %s |\n' "$p" "$u"
		done <"$OUT/lote.tsv"
		printf '\n'
	fi
	if [ -s "$OUT/failed.txt" ]; then
		printf '### Fallos de generación\n\n'
		while IFS= read -r u; do printf -- '- %s\n' "$u"; done <"$OUT/failed.txt"
	fi
} >"$OUT/summary.md"
