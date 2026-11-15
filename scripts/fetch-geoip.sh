#!/bin/sh
# Download the free DB-IP Lite country and ASN databases (CC BY 4.0, https://db-ip.com)
# into a folder, for the news server host lookup. RustyGet reads them locally, so no
# server address is ever sent anywhere to be located.
# Usage: scripts/fetch-geoip.sh DEST_DIR
# A failed download only warns: host lookup then shows addresses and connect times
# without country and network.
set -u
dest="${1:?usage: fetch-geoip.sh DEST_DIR}"
mkdir -p "$dest"
status=0
year=$(date -u +%Y)
month=$(date -u +%m)
prev_month=$(expr "$month" - 1)
prev_year=$year
if [ "$prev_month" -eq 0 ]; then prev_month=12; prev_year=$(expr "$year" - 1); fi
this_month="$year-$month"
last_month=$(printf '%s-%02d' "$prev_year" "$prev_month")
for kind in country asn; do
    file="$dest/dbip-$kind-lite.mmdb"
    got=no
    # DB-IP publishes one file a month; early in a month only last month's may exist.
    for stamp in "$this_month" "$last_month"; do
        url="https://download.db-ip.com/free/dbip-$kind-lite-$stamp.mmdb.gz"
        if curl -fsSL --retry 2 "$url" -o "$file.gz.part" && gunzip -c "$file.gz.part" > "$file.part" && [ -s "$file.part" ]; then
            mv "$file.part" "$file"
            got=yes
            break
        fi
    done
    rm -f "$file.gz.part" "$file.part"
    if [ "$got" = yes ]; then
        echo "fetch-geoip: $file"
    else
        echo "fetch-geoip: warning: could not download the DB-IP $kind database" >&2
        status=1
    fi
done
[ "$status" = 0 ] && printf '%s\n' "IP geolocation by DB-IP (https://db-ip.com), licensed CC BY 4.0." > "$dest/NOTICE"
exit 0
