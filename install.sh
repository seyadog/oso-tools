#!/bin/sh
# Installs oso for the current user (no root): the program in ~/.local/share/oso, a link `oso` in ~/.local/bin and
# an entry in the applications menu. Linux x86_64 only.
#
#   curl -fsSL https://raw.githubusercontent.com/seyadog/oso-tools/main/install.sh | sh
#
# Take a look at the script before running it if you like: it is short, it only writes under your home folder and
# it downloads one file (and its checksum) from the GitHub release of this repository.
# `OSO_VERSION=v0.4.0 sh install.sh` installs a given release instead of the latest one.
set -eu

repo="seyadog/oso-tools"
version="${OSO_VERSION:-latest}"
prefix="${XDG_DATA_HOME:-$HOME/.local/share}/oso"
bin="$HOME/.local/bin"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

say() { printf '%s\n' "$*"; }
die() { printf 'oso install: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || die "oso only runs on Linux."
case "$(uname -m)" in
  x86_64 | amd64) ;;
  *) die "there is only a build for x86_64 (this computer is $(uname -m)). You can build it: see the README." ;;
esac
command -v curl >/dev/null 2>&1 || die "curl is needed."
command -v tar >/dev/null 2>&1 || die "tar is needed."
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is needed."

if [ "$version" = "latest" ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/$version"
fi
file="oso-linux-x86_64.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "Downloading oso ($version)…"
curl -fsSL "$base/$file" -o "$tmp/$file" || die "could not download $base/$file (is there a release yet?)"
curl -fsSL "$base/$file.sha256" -o "$tmp/$file.sha256" || die "could not download the checksum"

say "Checking the download…"
(cd "$tmp" && sha256sum -c "$file.sha256" >/dev/null) || die "the checksum does not match: nothing was installed."

mkdir -p "$prefix" "$bin" "$apps"
tar -xzf "$tmp/$file" -C "$tmp"
dir="$tmp/oso-linux-x86_64"
[ -f "$dir/oso" ] || die "the download does not look right."

install -m755 "$dir/oso" "$prefix/oso.new"
mv -f "$prefix/oso.new" "$prefix/oso"
install -m644 "$dir/oso.png" "$prefix/oso.png"
ln -sf "$prefix/oso" "$bin/oso"

cat > "$apps/oso.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=oso
Comment=Secrets vault and terminal workspace
Exec=$prefix/oso
Icon=$prefix/oso.png
Terminal=false
Categories=Utility;System;TerminalEmulator;
StartupWMClass=oso
EOF
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" >/dev/null 2>&1 || true

say ""
say "oso is installed: $prefix/oso (and the command \`oso\`)."
case ":$PATH:" in
  *":$bin:"*) ;;
  *) say "Add $bin to your PATH to use the command \`oso\` from a terminal." ;;
esac

missing=""
if command -v ldd >/dev/null 2>&1; then
  missing="$(ldd "$prefix/oso" 2>/dev/null | grep 'not found' | awk '{print $1}' | tr '\n' ' ' || true)"
fi
if [ -n "$missing" ]; then
  say ""
  say "It needs some system libraries that are not installed yet: $missing"
  say "  Fedora:        sudo dnf install webkit2gtk4.1 gtk3 libsoup3"
  say "  Debian/Ubuntu: sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libsoup-3.0-0"
fi
say ""
say "Open it from the applications menu, or run: oso"
say "If an older oso is open, close it and open it again to use this version."
