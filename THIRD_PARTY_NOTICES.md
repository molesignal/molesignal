# Third-Party Notices

## Grafana UI visualization primitives

MoleSignal's dashboard visualizations adapt portions of the layout and geometry
approaches from Grafana UI's Apache-2.0 component package.

- Upstream project: [Grafana](https://github.com/grafana/grafana)
- Upstream version: `v13.1.0`
- Upstream commit: `b309c9bb3b81a748c3a75289236a27309ed2566a`
- Adapted sources:
  - `packages/grafana-ui/src/components/RadialGauge/utils.ts`
  - `packages/grafana-ui/src/components/BigValue/BigValue.tsx`
  - `packages/grafana-ui/src/components/BigValue/BigValueLayout.tsx`
  - `packages/grafana-ui/src/components/BarGauge/BarGauge.tsx`
  - `packages/grafana-ui/src/components/Sparkline/Sparkline.tsx`
  - `packages/grafana-ui/src/utils/measureText.ts`
- License: Apache License 2.0, under Grafana's `packages/grafana-ui/`
  licensing exception documented in upstream `LICENSING.md`
- Copyright 2015 Grafana Labs

The Apache License 2.0 text is included in this repository's root `LICENSE`
file.

MoleSignal modifications include replacing Grafana data and theme contracts
with local `DataFrame` and `FieldConfig` contracts, implementing local SVG and
DOM renderers, adding bounded-data behavior, stable zero-width ranges,
normalized thresholds, responsive layouts, accessibility semantics, and tests.
The Bar Chart, Heatmap, and State Timeline implementations are original local
work. No source from Grafana's AGPL-licensed `public/app/plugins/panel/`
directory is included.

## DB-IP IP to City Lite database

MoleSignal fills the `country` of RUM events from the client IP with DB-IP's
IP to City Lite database. The database is not stored in this repository:
`scripts/fetch-dbip-city-lite.sh` downloads the current monthly edition when the
container images and the release archives are built, and it ships unmodified
at `/usr/share/molesignal/mmdb/` in the images and under `mmdb/` in the
archives, each time with `assets/mmdb/NOTICE.txt` beside it.

IP Geolocation by DB-IP: https://db-ip.com

- Upstream: [DB-IP IP to City Lite](https://db-ip.com/db/download/ip-to-city-lite)
- License: [Creative Commons Attribution 4.0 International License](https://creativecommons.org/licenses/by/4.0/)
- The RUM pages, which show results from the database, link to DB-IP.com as
  DB-IP's terms for web applications ask.

### Earlier revisions

Revisions of this repository before the switch to DB-IP contain MaxMind's
GeoLite City database at `assets/mmdb/GeoLite2-City.mmdb`. This product
includes GeoLite Data created by MaxMind, available from
https://www.maxmind.com. That database is governed by the
[GeoLite End User License Agreement](https://www.maxmind.com/en/geolite/eula);
its copyrightable elements are licensed under the
[Creative Commons Attribution-ShareAlike 4.0 International License](https://creativecommons.org/licenses/by-sa/4.0/),
and it incorporates [GeoNames](https://www.geonames.org) geographical data made
available under the
[Creative Commons Attribution 4.0 International License](https://creativecommons.org/licenses/by/4.0/).
