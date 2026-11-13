#!/usr/bin/env bash
# Fail when a native library has an ELF LOAD segment aligned below 16 KB.
#
# Android devices with 16 KB memory pages refuse (or run in a compat mode with a
# warning dialog) libraries whose LOAD segments are 4 KB aligned, and Google Play
# requires 16 KB support for apps targeting Android 15+:
# https://developer.android.com/guide/practices/page-sizes
#
# Usage: scripts/check-elf-alignment.sh <path>...
#   <path> is an .aab/.apk (extracted to a temp dir), a directory searched
#   recursively for *.so, or a single .so file.
#
# Exits 1 if any LOAD segment is below 16384, or if no .so file was found
# (an empty check must not pass). Set READELF to override the readelf binary.
set -euo pipefail

readonly MIN_ALIGN=16384

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <aab|apk|directory|.so>..." >&2
  exit 2
fi

readelf_bin="${READELF:-}"
if [[ -z "$readelf_bin" ]]; then
  readelf_bin="$(command -v readelf || command -v llvm-readelf || true)"
fi
if [[ -z "$readelf_bin" ]]; then
  echo "error: readelf or llvm-readelf not found (set READELF)" >&2
  exit 2
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

# Collect the .so files behind each argument, one path per line.
libs_file="$workdir/libs.txt"
: > "$libs_file"
index=0
for input in "$@"; do
  if [[ -d "$input" ]]; then
    find "$input" -type f -name '*.so' >> "$libs_file"
  elif [[ "$input" == *.so ]]; then
    [[ -f "$input" ]] || { echo "error: $input not found" >&2; exit 2; }
    echo "$input" >> "$libs_file"
  elif [[ -f "$input" ]]; then
    index=$((index + 1))
    extract_dir="$workdir/extract-$index"
    mkdir -p "$extract_dir"
    unzip -q -o "$input" '*.so' -d "$extract_dir"
    find "$extract_dir" -type f -name '*.so' >> "$libs_file"
  else
    echo "error: $input not found" >&2
    exit 2
  fi
done

if [[ ! -s "$libs_file" ]]; then
  echo "error: no .so files found in: $*" >&2
  exit 1
fi

failed=0
while IFS= read -r lib; do
  # In `readelf -lW` output the alignment is the last column of each LOAD row.
  aligns="$("$readelf_bin" -lW "$lib" | awk '$1 == "LOAD" { print $NF }')"
  if [[ -z "$aligns" ]]; then
    echo "FAIL  ${lib#"$workdir"/} (no LOAD segments read)"
    failed=1
    continue
  fi
  min=""
  for align in $aligns; do
    value=$((align))
    if [[ -z "$min" || "$value" -lt "$min" ]]; then
      min="$value"
    fi
  done
  if [[ "$min" -lt "$MIN_ALIGN" ]]; then
    echo "FAIL  ${lib#"$workdir"/} (min LOAD align $min < $MIN_ALIGN)"
    failed=1
  else
    echo "ok    ${lib#"$workdir"/} (min LOAD align $min)"
  fi
done < "$libs_file"

if [[ "$failed" -ne 0 ]]; then
  echo "error: native libraries are not 16 KB page-aligned" >&2
  exit 1
fi
