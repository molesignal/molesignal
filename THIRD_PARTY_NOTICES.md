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
