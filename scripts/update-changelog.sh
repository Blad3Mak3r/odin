#!/bin/sh
set -eu

version=${1:-}
release_date=${2:-}
notes_file=${3:-}
changelog=${4:-CHANGELOG.md}

if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "usage: $0 VERSION YYYY-MM-DD NOTES_FILE [CHANGELOG_FILE]" >&2
    exit 1
fi

if ! printf '%s\n' "$release_date" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'; then
    echo "usage: $0 VERSION YYYY-MM-DD NOTES_FILE [CHANGELOG_FILE]" >&2
    exit 1
fi

if [ ! -s "$notes_file" ]; then
    echo "release notes file is missing or empty: $notes_file" >&2
    exit 1
fi

if ! grep -Fqx '## [Unreleased]' "$changelog"; then
    echo "missing ## [Unreleased] heading in $changelog" >&2
    exit 1
fi

if grep -Fq "## [$version]" "$changelog"; then
    echo "version $version already exists in $changelog" >&2
    exit 1
fi

tmp=$(mktemp "${changelog}.XXXXXX")
trap 'rm -f "$tmp"' EXIT

awk -v version="$version" -v release_date="$release_date" -v notes_file="$notes_file" '
    $0 == "## [Unreleased]" {
        print
        print ""
        print "## [" version "] - " release_date
        print ""
        while ((getline line < notes_file) > 0) {
            print line
        }
        close(notes_file)
        next
    }
    { print }
' "$changelog" > "$tmp"

mv "$tmp" "$changelog"
trap - EXIT
