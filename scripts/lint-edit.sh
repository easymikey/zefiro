#!/bin/sh
set -u

case "$1" in
    *.rs) ;;
    *) exit 0 ;;
esac

if git ls-files --error-unmatch "$1" >/dev/null 2>&1; then
    sh scripts/rules.sh main "$1"
else
    git diff --no-index --unified=0 /dev/null "$1" | rg '^\+[^+]' \
        | rg --pcre2 --max-count 20 '^\+\s*//(?! SAFETY:)|^\+.*\S\s+//(?! SAFETY:)|super::|pub\(super\)|unwrap_or_default|#!?\[(allow|expect)\(' \
        | sed 's/^/rule: /'
    lines=$(wc -l < "$1")
    [ "$lines" -gt 800 ] && printf 'length: %s has %s lines\n' "$1" "$lines"
fi
exit 0
