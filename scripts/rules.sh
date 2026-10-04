#!/bin/sh
set -u

base=$(git merge-base "${1:-main}" HEAD)
paths="${2:-crates/*.rs}"
status=0

added=$(git diff "$base" --unified=0 -- "$paths" | rg '^\+[^+]' || true)

check() {
    found=$(printf '%s\n' "$added" | rg --pcre2 --max-count 20 "$1" || true)
    if [ -n "$found" ]; then
        printf '%s\n' "$found" | sed "s/^/$2: /"
        status=1
    fi
}

check '^\+\s*//(?! SAFETY:)|^\+.*\S\s+//(?! SAFETY:)' 'comment'
check 'super::|pub\(super\)' 'super'
check 'unwrap_or_default' 'unwrap_or_default'
check '#!?\[(allow|expect)\(' 'allow'
check '\blet _\b' 'let _'
check '\.ok\(\);' 'ok'

for file in $(git diff "$base" --name-only --diff-filter=AM -- "$paths"); do
    lines=$(wc -l < "$file")
    if [ "$lines" -gt 800 ]; then
        printf 'length: %s has %s lines\n' "$file" "$lines"
        status=1
    fi
done

whole_tree=$(rg -l "super::|pub\(super\)" crates/*/src || true)
if [ -n "$whole_tree" ]; then
    printf '%s\n' "$whole_tree" | sed 's/^/super (whole tree): /'
    status=1
fi

exit "$status"
