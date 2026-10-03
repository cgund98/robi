#!/usr/bin/env bash
#
# Audit relative links in the mdBook sources under docs/src/.
#
# A "relative link" is a markdown link whose target is not http(s), mailto:,
# tel:, or a bare #fragment. Each target is resolved against the page's
# directory and must name an existing file (a #fragment is ignored). This
# catches the links a file move breaks. Run by `make lint`; exits non-zero on
# any broken link.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/docs/src"

broken=0

while IFS= read -r file; do
  dir="$(dirname "$file")"
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    lineno="${entry%%:*}"
    match="${entry#*:}"

    # Strip the leading "]( " and trailing ")".
    target="${match#](}"
    target="${target%)}"

    # Drop an optional title ("..." or '...') and any surrounding <>.
    target="${target%%[[:space:]]*}"
    target="${target#<}"
    target="${target%>}"

    case "$target" in
      ""|\#*|http://*|https://*|mailto:*|tel:*) continue ;;
    esac

    # Split off a #fragment.
    base="${target%%#*}"
    [ -n "$base" ] || continue

    # Resolve against the page's directory without realpath.
    resolved_dir="$(cd "$dir" 2>/dev/null && cd "$(dirname "$base")" 2>/dev/null && pwd)" || {
      printf '%s:%s: broken link -> %s\n' "${file#"$ROOT"/}" "$lineno" "$target"
      broken=$((broken + 1))
      continue
    }
    resolved="$resolved_dir/$(basename "$base")"

    # Links that escape the repo (sibling checkouts) are external.
    case "$resolved" in
      "$ROOT"/*) ;;
      *) continue ;;
    esac

    if [ ! -e "$resolved" ]; then
      printf '%s:%s: broken link -> %s\n' "${file#"$ROOT"/}" "$lineno" "$target"
      broken=$((broken + 1))
    fi
  done < <(grep -noE '\]\([^)]*\)' "$file")
done < <(find "$SRC" -name '*.md' | sort)

if [ "$broken" -ne 0 ]; then
  printf '\n%s broken relative link(s).\n' "$broken"
  exit 1
fi
printf 'All relative doc links resolve.\n'
