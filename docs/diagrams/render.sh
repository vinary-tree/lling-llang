#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# render.sh — render every diagram source under docs/diagrams/ to a sibling SVG.
#
# Dispatch by file extension (the extension selects the engine):
#   *.dot   → Graphviz   (dot -Tsvg)
#   *.puml  → PlantUML    (plantuml -checkonly  then  plantuml -tsvg)
#   *.d2    → D2          (d2 --layout elk)
#   *.mmd   → Mermaid     (mmdc)
#   *.tex   → TikZ/PGF    (lualatex → DVI → dvisvgm)
#
# Idempotent: a source is re-rendered only when it is newer than its .svg
# (override with --force). Deterministic: re-running with no source changes
# writes nothing, so CI can assert "no tracked SVG drifted".
#
# Usage:
#   docs/diagrams/render.sh [--force] [--check] [path-or-dir ...]
#     --force   re-render every matched source unconditionally
#     --check   validate sources only; write NO svg (CI gate)
#     (no args) process the whole docs/diagrams/ tree
#
# All engines are local (the pgmcp Kroki gateway at :8000 is optional and not
# required here). See docs/diagrams/README.md for the authoring conventions.
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"   # docs/diagrams
FORCE=0
CHECK=0
declare -a TARGETS=()

for a in "$@"; do
  case "$a" in
    --force) FORCE=1 ;;
    --check) CHECK=1 ;;
    --help|-h) sed -n '2,33p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) TARGETS+=("$a") ;;
  esac
done
[ ${#TARGETS[@]} -eq 0 ] && TARGETS=("$DIR")

need() { command -v "$1" >/dev/null 2>&1 || { echo "MISSING engine: $1 (skipping $2)" >&2; return 1; }; }
newer() { [ "$FORCE" -eq 1 ] || [ ! -f "$2" ] || [ "$1" -nt "$2" ]; }   # is src newer than svg?

render_one() {
  local src="$1" svg="${1%.*}.svg" ext="${1##*.}" rc=0
  if ! newer "$src" "$svg"; then
    echo "skip   $src"
    return 0
  fi
  case "$ext" in
    dot)  need dot "$src"      || return 0; dot -Tsvg "$src" -o "$svg" ;;
    puml) need plantuml "$src" || return 0; local outdir; outdir="$(cd "$(dirname "$src")" && pwd)";
          # DISPLAY= forces AWT headless (no X11 needed); -o is absolute so the SVG
          # lands as a sibling regardless of whether $src is a relative or absolute arg.
          DISPLAY= plantuml -checkonly "$src" && DISPLAY= plantuml -tsvg -o "$outdir" "$src" ;;
    d2)   need d2 "$src"       || return 0; d2 --layout elk "$src" "$svg" ;;
    mmd)  need mmdc "$src"     || return 0; mmdc --quiet -i "$src" -o "$svg" -b transparent ;;
    tex)  need lualatex "$src" || return 0; need dvisvgm "$src" || return 0; (
            tmp="$(mktemp -d)"; cp "$src" "$tmp/fig.tex"
            ( cd "$tmp" && lualatex --interaction=nonstopmode --halt-on-error --output-format=dvi fig.tex >/dev/null 2>&1 )
            dvisvgm --no-fonts --exact --output="$svg" "$tmp/fig.dvi" >/dev/null 2>&1
            rm -rf "$tmp"
          ) ;;
    svg)  return 0 ;;
    *)    echo "unknown extension: $src" >&2; return 1 ;;
  esac || rc=$?
  if [ $rc -eq 0 ] && [ -f "$svg" ]; then echo "render $src → $svg"; else echo "FAIL   $src (rc=$rc)" >&2; return 1; fi
}

validate_one() {   # --check: parse/validate only, emit no SVG
  local src="$1" ext="${1##*.}"
  case "$ext" in
    puml) need plantuml "$src" || return 0; DISPLAY= plantuml -checkonly "$src" ;;
    dot)  need dot "$src"      || return 0; dot -Tcanon "$src" >/dev/null ;;
    d2)   need d2 "$src"       || return 0; d2 fmt "$src" >/dev/null ;;
    mmd)  need mmdc "$src"     || return 0; mmdc --quiet -i "$src" -o /dev/null >/dev/null 2>&1 ;;
    tex)  need lualatex "$src" || return 0; local t r; t="$(mktemp -d)"; cp "$src" "$t/fig.tex";
          ( cd "$t" && lualatex --interaction=nonstopmode --halt-on-error --output-format=dvi fig.tex >/dev/null 2>&1 ); r=$?;
          rm -rf "$t"; return $r ;;
    svg)  return 0 ;;
  esac
}

status=0
while IFS= read -r -d '' src; do
  if [ "$CHECK" -eq 1 ]; then
    if validate_one "$src"; then echo "ok     $src"; else echo "INVALID $src" >&2; status=1; fi
  else
    render_one "$src" || status=1
  fi
done < <(
  for t in "${TARGETS[@]}"; do
    if [ -d "$t" ]; then
      find "$t" -type f \( -name '*.dot' -o -name '*.puml' -o -name '*.d2' -o -name '*.mmd' -o -name '*.tex' \) -print0
    else
      printf '%s\0' "$t"
    fi
  done | sort -z
)

exit $status
