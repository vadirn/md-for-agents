#!/bin/sh
set -eu
binaries=$1
output=$2
mkdir -p "$output"
for fixture in mdread/tests/fixtures/*.md; do
  name=$(basename "$fixture" .md)
  "$binaries/mdstruct" - < "$fixture" > "$output/$name.mdstruct.stdout" 2> "$output/$name.mdstruct.stderr"
  "$binaries/mdread" "$fixture" --format json > "$output/$name.mdread.stdout" 2> "$output/$name.mdread.stderr"
  # Text readings from stdin, as mdread.wasm reads. A case may fail, so its exit
  # code is recorded instead of stopping the run.
  grep -Ev '^(#|$)' scripts/mdread-cases.txt | while read -r label args; do
    out="$output/$name.mdread-$label"
    # $args is unquoted so that it splits into separate arguments.
    "$binaries/mdread" - $args < "$fixture" > "$out.stdout" 2> "$out.stderr" \
      && echo 0 > "$out.exit" || echo $? > "$out.exit"
  done
done
