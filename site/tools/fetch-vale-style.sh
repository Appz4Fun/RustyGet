#!/bin/sh
# Downloads the Google style for Vale into site/vale/styles, at a pinned version
# and checksum. Run it from the repository root before running Vale.
set -eu

version=0.7.1
sha256=4b67fca1f2a88595b2a578ab98bbf3017a23af677b04f93e862843f3ea1a9a0b
styles=site/vale/styles

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl --fail --location --silent --show-error --output "$work/Google.zip" \
	"https://github.com/vale-cli/Google/releases/download/v$version/Google.zip"
if command -v sha256sum >/dev/null 2>&1; then
	echo "$sha256  $work/Google.zip" | sha256sum --check --quiet
else
	echo "$sha256  $work/Google.zip" | shasum -a 256 --check --quiet
fi
unzip -q "$work/Google.zip" -d "$work/style"
rm -rf "$styles/Google"
mv "$work/style/Google" "$styles/Google"
