#!/bin/sh
# Build the side-by-side `rustyget` .deb on a Debian/Ubuntu host of the target arch.
# Needs: build-essential pkg-config libssl-dev python3 just dpkg-dev fakeroot, rustup.
# Usage: linux/pkg/deb-rustyget/build-deb.sh [jobs]   -> build/deb/rustyget_<ver>_<arch>.deb
set -eu

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
PKG="$ROOT/linux/pkg/rustyget"
JOBS=${1:-3}
cd "$ROOT"

just release "$JOBS"

ARCH=$(dpkg --print-architecture)
BASE=$(sed -n 's/^version = "\(.*\)"/\1/p' rust/Cargo.toml | head -1)
REV=$(git rev-parse --short=8 HEAD 2>/dev/null || echo unknown)
VERSION="${BASE}+git$(git log -1 --format=%cd --date=format:%Y%m%d 2>/dev/null || date +%Y%m%d).${REV}"

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
DEPS=$(cd "$SHL" && dpkg-shlibdeps -O "$STAGE/usr/bin/rustyget" 2>/dev/null | sed -n 's/^shlibs:Depends=//p')
rm -rf "$SHL"
DEPENDS="adduser, systemd, ${DEPS}"

sed -e "s|@VERSION@|$VERSION|" -e "s|@ARCH@|$ARCH|" -e "s|@DEPENDS@|$DEPENDS|" \
    "$PKG/DEBIAN/control" > "$STAGE/DEBIAN/control"
install -m 0755 "$PKG/DEBIAN/postinst" "$PKG/DEBIAN/prerm" "$PKG/DEBIAN/postrm" "$STAGE/DEBIAN/"

chmod -R u+rwX,go+rX,go-w "$STAGE/usr" "$STAGE/lib"
OUT="$ROOT/build/deb/rustyget_${VERSION}_${ARCH}.deb"
fakeroot dpkg-deb -Zxz --build "$STAGE" "$OUT"
echo "$OUT"
