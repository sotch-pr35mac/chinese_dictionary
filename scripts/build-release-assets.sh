#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 SOURCE_DATA_DIR [OUTPUT_DIR]" >&2
    exit 2
fi
source_dir=$(CDPATH= cd -- "$1" && pwd)
output_arg=${2:-"$repo_dir/dist"}
mkdir -p "$output_arg"
output_dir=$(CDPATH= cd -- "$output_arg" && pwd)
stage_dir=$(mktemp -d "${TMPDIR:-/tmp}/chinese-dictionary-4.1.0.XXXXXX")
trap 'rm -rf "$stage_dir"' EXIT HUP INT TERM

for name in \
    dictionary.rkyv.zst \
    english.search.zst \
    wiktionary-attribution.json \
    manifest.json \
    NOTICE.md \
    LICENSE-DATA.txt \
    LICENSE-WORDNET.txt
do
    cp "$source_dir/$name" "$stage_dir/$name"
    chmod 0644 "$stage_dir/$name"
    TZ=UTC touch -t 202609190000.00 "$stage_dir/$name"
done

bundle="$output_dir/chinese_dictionary-data-4.1.0.tar.gz"
attribution="$output_dir/wiktionary-attribution-4.1.0.json"
uncompressed_bundle="$stage_dir/chinese_dictionary-data-4.1.0.tar"
COPYFILE_DISABLE=1 tar -cf "$uncompressed_bundle" \
    --format ustar --uid 0 --gid 0 --uname root --gname root \
    -C "$stage_dir" \
    dictionary.rkyv.zst \
    english.search.zst \
    wiktionary-attribution.json \
    manifest.json \
    NOTICE.md \
    LICENSE-DATA.txt \
    LICENSE-WORDNET.txt
gzip -n -9 -c "$uncompressed_bundle" > "$bundle"
cp "$stage_dir/wiktionary-attribution.json" "$attribution"

(
    cd "$output_dir"
    shasum -a 256 \
        chinese_dictionary-data-4.1.0.tar.gz \
        wiktionary-attribution-4.1.0.json > SHA256SUMS
    shasum -a 256 -c SHA256SUMS
)

echo "Release assets written to $output_dir"
