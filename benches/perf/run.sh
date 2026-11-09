#!/usr/bin/env bash
# Benchmarks: oitec (native, --release) against Bun and Node, best of N runs.
#
#   benches/perf/run.sh [--oitec PATH]... [--runs N] [bench...]
#
# Each --oitec adds a column (e.g. master and a branch). Bun and Node run the
# same source as TypeScript.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
runs=3
oitecs=()
benches=()
while [ $# -gt 0 ]; do
  case "$1" in
    --oitec) oitecs+=("$2"); shift 2 ;;
    --runs) runs="$2"; shift 2 ;;
    *) benches+=("$1"); shift ;;
  esac
done
[ ${#oitecs[@]} -eq 0 ] && oitecs=("$here/../../target/release/oitec")
[ ${#benches[@]} -eq 0 ] && benches=($(cd "$here" && ls *.ot | sed 's/\.ot$//'))
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

best() { # best wall time in ms of N runs of a command; output in $work/out
  local min=""
  for _ in $(seq "$runs"); do
    local s e
    s=$(date +%s%N)
    if ! timeout 300 "$@" >"$work/out" 2>&1; then echo "fail"; return; fi
    e=$(date +%s%N)
    local t=$(( (e - s) / 1000000 ))
    if [ -z "$min" ] || [ "$t" -lt "$min" ]; then min=$t; fi
  done
  echo "$min"
}

printf "%-14s" bench
for i in "${!oitecs[@]}"; do printf "%10s" "oite$i"; done
printf "%10s%10s  %s\n" bun node result
for b in "${benches[@]}"; do
  src="$here/$b.ot"
  cp "$src" "$work/$b.ts"
  printf "%-14s" "$b"
  result=""
  for i in "${!oitecs[@]}"; do
    exe="$work/$b.$i"
    if "${oitecs[$i]}" build "$src" --release -o "$exe" >"$work/build.log" 2>&1; then
      printf "%10s" "$(best "$exe")"
      result="$(head -1 "$work/out")"
    else
      printf "%10s" "nobuild"
    fi
  done
  printf "%10s" "$(best bun "$work/$b.ts")"
  bun_result="$(head -1 "$work/out")"
  printf "%10s" "$(best node --experimental-strip-types --no-warnings "$work/$b.ts")"
  node_result="$(head -1 "$work/out")"
  mark=""
  [ -n "$result" ] && [ "$result" != "$bun_result" ] && mark=" (bun: $bun_result)"
  printf "  %s%s\n" "$result" "$mark"
done
