## Platforms

RustyGet builds from source on Linux and macOS with `just` and Rust. See [Build and run](README.md#build-and-run).

## Server location data

The news server host lookup in Settings shows each address's country and network
from the free [DB-IP Lite](https://db-ip.com) databases (CC BY 4.0). They are read
from local files, so no server address is ever sent anywhere to be located. The
Debian package and the Docker image include them; for another install, run
`scripts/fetch-geoip.sh DIR` and put `dbip-country-lite.mmdb` and
`dbip-asn-lite.mmdb` in `geoip/` next to the `rustyget` binary,
`/usr/share/rustyget/geoip/`, or a folder named by `RUSTYGET_GEOIP_DIR`. Without
them the lookup still lists addresses and connect times.
