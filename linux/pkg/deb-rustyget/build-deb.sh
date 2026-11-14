#!/bin/sh
# Build the side-by-side `rustyget` .deb on a Debian/Ubuntu host of the target arch.
# Needs: build-essential pkg-config libssl-dev python3 just dpkg-dev, rustup.
# Usage: linux/pkg/deb-rustyget/build-deb.sh [jobs]   -> build/deb/rustyget_<ver>_<arch>.deb
set -eu

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
PKG=$(cd "$(dirname "$0")" && pwd)
JOBS=${1:-3}
cd "$ROOT"

just release "$JOBS"

ARCH=$(dpkg --print-architecture)
BASE=$(sed -n 's/^version = "\(.*\)"/\1/p' rust/Cargo.toml | head -1)
# RG_VERSION overrides (e.g. when building from a copy without .git).
if [ -z "${RG_VERSION:-}" ] && git rev-parse HEAD >/dev/null 2>&1; then
    # Commit time (UTC) first so newer commits always sort as upgrades; hash last for identification.
    RG_VERSION="${BASE}+git$(TZ=UTC git log -1 --format=%cd --date=format-local:%Y%m%d%H%M%S).$(git rev-parse --short=8 HEAD)"
fi
VERSION=${RG_VERSION:-${BASE}}

STAGE="$ROOT/build/deb/stage"
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" "$STAGE/usr/bin" "$STAGE/usr/share/rustyget" \
         "$STAGE/usr/share/doc/rustyget" "$STAGE/lib/systemd/system"

install -m 0755 build/release/rustyget "$STAGE/usr/bin/rustyget"
cp -R webui "$STAGE/usr/share/rustyget/webui"
install -m 0644 ChangeLog.md COPYING "$STAGE/usr/share/doc/rustyget/"
install -m 0644 "$PKG/rustyget.service" "$STAGE/lib/systemd/system/rustyget.service"

# Packaged template: never collides with a stock nzbget install (port 6789/6791, /var/lib/nzbget).
sed -e 's|^MainDir=.*|MainDir=/var/lib/rustyget/downloads|' \
    -e 's|^WebDir=.*|WebDir=/usr/share/rustyget/webui|' \
    -e 's|^ConfigTemplate=.*|ConfigTemplate=/usr/share/rustyget/rustyget.conf|' \
    -e 's|^ControlPort=.*|ControlPort=6790|' \
    -e 's|^SecurePort=.*|SecurePort=6792|' \
    -e 's|^LockFile=.*|LockFile=${MainDir}/rustyget.lock|' \
    -e 's|^UnrarCmd=.*|UnrarCmd=unrar|' \
    -e 's|^SevenZipCmd=.*|SevenZipCmd=7zz|' \
    -e 's|^AuthorizedIP=.*|AuthorizedIP=127.0.0.1|' \
    -e 's|^UpdateCheck=.*|UpdateCheck=none|' \
    -e 's|^UMask=.*|UMask=0002|' \
    nzbget.conf > "$STAGE/usr/share/rustyget/rustyget.conf"
chmod 0644 "$STAGE/usr/share/rustyget/rustyget.conf"

# Shared-library dependencies from the binary itself.
SHL=$(mktemp -d)
mkdir -p "$SHL/debian"
printf 'Source: rustyget\n\nPackage: rustyget\nArchitecture: any\n' > "$SHL/debian/control"
if ! SHLIBS=$(cd "$SHL" && dpkg-shlibdeps -O "$STAGE/usr/bin/rustyget"); then
    rm -rf "$SHL"
    echo "dpkg-shlibdeps failed; refusing to build a package without library dependencies" >&2
    exit 1
fi
rm -rf "$SHL"
DEPS=$(printf '%s\n' "$SHLIBS" | sed -n 's/^shlibs:Depends=//p')
if [ -z "$DEPS" ]; then
    echo "dpkg-shlibdeps reported no dependencies for rustyget; refusing to build" >&2
    exit 1
fi
DEPENDS="adduser, systemd, ${DEPS}"

sed -e "s|@VERSION@|$VERSION|" -e "s|@ARCH@|$ARCH|" -e "s|@DEPENDS@|$DEPENDS|" \
    "$PKG/DEBIAN/control" > "$STAGE/DEBIAN/control"
install -m 0755 "$PKG/DEBIAN/postinst" "$PKG/DEBIAN/prerm" "$PKG/DEBIAN/postrm" "$STAGE/DEBIAN/"

chmod -R u+rwX,go+rX,go-w "$STAGE/usr" "$STAGE/lib"
OUT="$ROOT/build/deb/rustyget_${VERSION}_${ARCH}.deb"
dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$OUT"
echo "$OUT"
