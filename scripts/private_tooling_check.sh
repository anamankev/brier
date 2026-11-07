#!/bin/sh
# Public repo, private tooling: no tracked file may mention the AI development tooling, and its
# files (assistant instructions and settings) must never be tracked.
# The patterns use character classes so that this script does not match itself.
set -eu
cd "$(git rev-parse --show-toplevel)"

status=0
if git grep -n -i -I -e 'c[l]aude' -e 'a[n]thropic' -- . ; then
    echo "private-tooling check: the tracked lines above mention the development tooling" >&2
    status=1
fi
tracked="$(git ls-files | grep -i -E '^(c[l]aude\.md$|\.c[l]aude/|docs/private/)' || true)"
if [ -n "$tracked" ]; then
    echo "private-tooling check: private files are tracked:" >&2
    echo "$tracked" >&2
    status=1
fi
[ "$status" -eq 0 ] && echo "private-tooling check: clean"
exit "$status"
