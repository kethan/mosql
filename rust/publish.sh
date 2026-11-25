#!/usr/bin/env bash
#
# Publish to crates.io, but only after every check passes.
#
#   ./publish.sh              # verify, then publish
#   ./publish.sh --dry-run    # verify, then cargo publish --dry-run
#
# Any extra arguments are forwarded to `cargo publish`.
#
set -euo pipefail
cd "$(dirname "$0")"

DRY_RUN=0
ARGS=()
for a in "$@"; do
  case "$a" in
    --dry-run) DRY_RUN=1 ;;
    *) ARGS+=("$a") ;;
  esac
done

# 1. Nothing passes that CI would not pass.
./verify.sh

# 2. Guard against publishing a build that is not the one just verified.
printf '\n\033[1m==> publishing\033[0m\n'
if [ "$DRY_RUN" -eq 1 ]; then
  # Forward extra arguments too, so `--allow-dirty` works for a local dry run.
  cargo publish --dry-run "${ARGS[@]+"${ARGS[@]}"}"
  printf '\nDry run only — nothing was uploaded.\n'
  exit 0
fi

# Refuse to publish a dirty tree: the published tarball is built from the
# working directory, so uncommitted changes — and untracked files matched by
# `include` in Cargo.toml — would ship without ever having been reviewed or
# tested. Scoped to this directory: unrelated files elsewhere in the repo are
# not the crate's concern (cargo's own check is scoped the same way).
if [ -n "$(git status --porcelain --untracked-files=all -- .)" ]; then
  echo "Refusing to publish: rust/ has uncommitted or untracked files:" >&2
  git --no-pager status --short --untracked-files=all -- . >&2
  echo "Commit them first — verify.sh tested the tree as it is, and the tarball" >&2
  echo "is built from exactly these files." >&2
  exit 1
fi

cargo publish "${ARGS[@]+"${ARGS[@]}"}"
