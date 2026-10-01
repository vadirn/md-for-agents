#!/bin/sh
set -eu
binaries=$1
output=$2
mkdir -p "$output"
for fixture in scripts/fixtures/*.md; do
  name=$(basename "$fixture" .md)
  "$binaries/mdstruct" - < "$fixture" > "$output/$name.mdstruct.stdout" 2> "$output/$name.mdstruct.stderr"
done
