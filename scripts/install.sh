#!/bin/sh
# Installs date-calc @VERSION@ into ~/.local/bin after checking the archive's SHA-256.
# curl -fsSL https://github.com/nowmyagentcan/date-calc/releases/latest/download/install.sh | sh
set -eu
version=@VERSION@
base=${DATE_CALC_BASE:-https://github.com/nowmyagentcan/date-calc/releases/download/$version}
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=linux-x64 ;;
  Linux-aarch64 | Linux-arm64) target=linux-arm64 ;;
  Darwin-x86_64) target=macos-x64 ;;
  Darwin-arm64) target=macos-arm64 ;;
  *) echo "date-calc: no build for $(uname -s) $(uname -m)." >&2; exit 1 ;;
esac
archive=date-calc-$version-$target.tar.gz
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/$archive" "$base/$archive"
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"
expected=$(awk -v f="$archive" '$2 == f { print $1 }' "$tmp/SHA256SUMS")
if command -v sha256sum >/dev/null; then actual=$(sha256sum "$tmp/$archive"); else actual=$(shasum -a 256 "$tmp/$archive"); fi
if [ -z "$expected" ] || [ "${actual%% *}" != "$expected" ]; then
  echo "date-calc: the SHA-256 of $archive doesn't match SHA256SUMS; nothing was installed." >&2
  exit 1
fi
tar -xzf "$tmp/$archive" -C "$tmp" date-calc
# An empty file "runs" as an empty script, so the answer is checked too.
case $("$tmp/date-calc" --version 2>/dev/null || :) in
  "date-calc "*) ;;
  *) echo "date-calc: $archive has no date-calc that runs here; nothing was installed." >&2; exit 1 ;;
esac
dir=$HOME/.local/bin
mkdir -p "$dir"
mv "$tmp/date-calc" "$dir/date-calc"
"$dir/date-calc" --version
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "Add $dir to your PATH: export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac
