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

## MaxMind GeoLite City database

MoleSignal fills the `country` of RUM events from the client IP with MaxMind's
GeoLite City database. The database is distributed unmodified in this
repository, in the container images at `/usr/share/molesignal/mmdb/`, and in
the release archives under `mmdb/`, each time with `NOTICE.txt` beside it.

This product includes GeoLite Data created by MaxMind, available from
https://www.maxmind.com.

- Upstream: [MaxMind GeoLite](https://dev.maxmind.com/geoip/geolite-free-geolocation-data)
- File: `assets/mmdb/GeoLite2-City.mmdb` (database type `GeoLite2-City`, built
  2026-05-29)
- License: [GeoLite End User License Agreement](https://www.maxmind.com/en/geolite/eula);
  its copyrightable elements are licensed under the
  [Creative Commons Attribution-ShareAlike 4.0 International License](https://creativecommons.org/licenses/by-sa/4.0/)
- Database and Contents Copyright (c) 2026 MaxMind, Inc.
- The database incorporates [GeoNames](https://www.geonames.org) geographical
  data, made available under the
  [Creative Commons Attribution 4.0 International License](https://creativecommons.org/licenses/by/4.0/)
