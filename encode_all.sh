#!/usr/bin/env bash
# encodes every media file in data/ into data/out/<name>.szt
# usage: ./encode_all.sh [--force]
#   --force  re-encode files that already have an szt in data/out/
# set OC_BIN to use another encoder binary

set -u

cd "$(dirname "$0")"

IN_DIR="data"
OUT_DIR="data/out"
FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1

SETTINGS=(-m single -f 10 -B polar-pair -b buffered -l 20% -D ciede --pcm)

if [ -z "${OC_BIN:-}" ]; then
	if command -v cargo >/dev/null 2>&1; then
		cargo build --release || exit 1
	fi
	if [ -x target/release/oc_img_gen.exe ]; then
		OC_BIN=target/release/oc_img_gen.exe
	else
		OC_BIN=target/release/oc_img_gen
	fi
fi
if [ ! -x "$OC_BIN" ]; then
	echo "encoder not found: $OC_BIN" >&2
	exit 1
fi

mkdir -p "$OUT_DIR"

# drop the half-written szt when ctrl+c stops an encode
out_path=""
trap '[ -n "$out_path" ] && rm -f "$out_path"; echo; exit 130' INT

done_count=0
skipped=0
failed=()

for in_path in "$IN_DIR"/*; do
	[ -f "$in_path" ] || continue
	file="$(basename "$in_path")"
	ext="${file##*.}"
	case "${ext,,}" in
		mp4|webm|mkv|mov|avi|gif|png|jpg|jpeg|webp|bmp) ;;
		*) continue ;;
	esac
	target="$OUT_DIR/${file%.*}.szt"

	if [ "$FORCE" -eq 0 ] && [ -e "$target" ]; then
		skipped=$((skipped + 1))
		continue
	fi

	out_path="$target"
	echo "== $file -> $out_path"
	if "$OC_BIN" encode "${SETTINGS[@]}" -i "$in_path" -o "$out_path"; then
		done_count=$((done_count + 1))
	else
		rm -f "$out_path"
		failed+=("$file")
	fi
	out_path=""
done

echo "encoded: $done_count, skipped (already in $OUT_DIR): $skipped, failed: ${#failed[@]}"
for file in "${failed[@]}"; do
	echo "  failed: $file"
done
[ "${#failed[@]}" -eq 0 ]
