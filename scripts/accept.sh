#!/bin/sh
set -u

brief="${1:-}"

if [ -z "$brief" ] || [ ! -f "$brief" ]; then
    echo "ACCEPT FAIL: missing brief file: $brief"
    exit 1
fi

if ! grep -q '^```acceptance[[:space:]]*$' "$brief"; then
    echo "ACCEPT SKIP: no acceptance block"
    exit 0
fi

block=$(mktemp)
owns=$(mktemp)
offenders=$(mktemp)
trap 'rm -f "$block" "$owns" "$offenders"' EXIT

awk '
    /^```acceptance[[:space:]]*$/ { infence = 1; next }
    infence && /^```[[:space:]]*$/ { infence = 0; next }
    infence { print }
' "$brief" > "$block"

status=0
has_owns=0
pending=""

report_fail() {
    echo "ACCEPT FAIL: $1"
    printf '%s\n' "$2" | head -n 20
}

while IFS= read -r line || [ -n "$line" ]; do
    [ -z "$line" ] && continue
    case "$line" in
        "CHECK: "*)
            pending=${line#CHECK: }
            ;;
        "EXPECT: "*)
            [ -z "$pending" ] && continue
            expect=${line#EXPECT: }
            output=$(sh -c "$pending" 2>&1)
            rc=$?
            if [ "$rc" -eq 0 ] && printf '%s' "$output" | grep -F -q -- "$expect"; then
                :
            else
                report_fail "$pending" "$output"
                status=1
            fi
            pending=""
            ;;
        EXPECT-EMPTY)
            [ -z "$pending" ] && continue
            output=$(sh -c "$pending" 2>&1)
            rc=$?
            if { [ "$rc" -eq 0 ] || [ "$rc" -eq 1 ]; } && [ -z "$output" ]; then
                :
            else
                report_fail "$pending" "$output"
                status=1
            fi
            pending=""
            ;;
        "OWNS: "*)
            has_owns=1
            printf '%s\n' "${line#OWNS: }" >> "$owns"
            ;;
    esac
done < "$block"

if [ "$has_owns" -eq 1 ]; then
    base=$(git merge-base main HEAD)
    : > "$offenders"
    git diff --name-only "$base" | while IFS= read -r file; do
        [ -z "$file" ] && continue
        matched=0
        while IFS= read -r glob; do
            [ -z "$glob" ] && continue
            case "$file" in
                $glob) matched=1; break ;;
            esac
        done < "$owns"
        [ "$matched" -eq 0 ] && printf '%s\n' "$file"
    done > "$offenders"
    if [ -s "$offenders" ]; then
        echo "ACCEPT FAIL: OWNS"
        head -n 20 "$offenders"
        status=1
    fi
fi

exit "$status"
