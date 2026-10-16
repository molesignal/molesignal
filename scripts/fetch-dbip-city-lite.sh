#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) 2026 MoleSignal Authors
#
# Download DB-IP's free IP to City Lite database (CC BY 4.0, https://db-ip.com)
# and write it, decompressed, to the given path. RUM fills the country of each
# event from it. The file is never committed: the release workflow, the
# container image and `make package` fetch the current monthly edition.
#
# Usage: scripts/fetch-dbip-city-lite.sh <output .mmdb path>
set -eu

out=${1:?usage: fetch-dbip-city-lite.sh <output .mmdb path>}
base=${DBIP_BASE_URL:-https://download.db-ip.com/free}

year=$(date -u +%Y)
month=$(date -u +%m)
month=${month#0}
current=$(printf '%d-%02d' "$year" "$month")

if [ "$month" -eq 1 ]; then
    previous=$(printf '%d-12' "$((year - 1))")
else
    previous=$(printf '%d-%02d' "$year" "$((month - 1))")
fi

mkdir -p "$(dirname "$out")"
download="$out.download.gz"
partial="$out.partial"
trap 'rm -f "$download" "$partial"' EXIT

# DB-IP publishes an edition on the first day of each month; until the current
# one is out, the previous one is the newest.
for edition in "$current" "$previous"; do
    url="$base/dbip-city-lite-$edition.mmdb.gz"

    if ! curl -fsSL --retry 3 --connect-timeout 20 -o "$download" "$url"; then
        echo "fetch-dbip-city-lite: $url not available" >&2
        continue
    fi

    gunzip -c "$download" > "$partial"

    # The metadata at the end of every MMDB file names its database type.
    if ! tail -c 131072 "$partial" | LC_ALL=C grep -a -q 'DBIP-City-Lite'; then
        echo "fetch-dbip-city-lite: $url is not a DB-IP City Lite database" >&2
        exit 1
    fi

    mv "$partial" "$out"
    echo "fetch-dbip-city-lite: edition $edition written to $out"
    exit 0
done

echo "fetch-dbip-city-lite: no edition found for $current or $previous at $base" >&2
exit 1
