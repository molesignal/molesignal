// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

/**
 * Builds the data of the world map, `web/src/viz/worldmap/world.json`: the countries of Natural
 * Earth, simplified, projected and written as the short SVG path strings the web app draws, so
 * that the app has nothing to decode or project when a map is first drawn.
 *
 * The source is Natural Earth's 1:10m `admin_0_countries` in the worldview of China (public
 * domain, https://github.com/nvkelso/natural-earth-vector/tree/master/geojson): Arunachal Pradesh
 * and Aksai Chin are China's, Kosovo is Serbia's, and Taiwan, Hong Kong and Macao are parts of
 * China. Taiwan is drawn on its own piece so that its rows can be coloured apart from the rest of
 * the country, with no border drawn about it. The source has no nine-dash line and no inset of
 * the South China Sea: this is not an approved map for publication in China.
 *
 * The tools are not dependencies of the web app; install them in a scratch directory:
 *
 *   mkdir world-map-tools && cd world-map-tools
 *   npm install topojson-server topojson-simplify topojson-client d3-geo
 *   curl -LO https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_10m_admin_0_countries_chn.geojson
 *   node <repo>/web/scripts/build-world-map.mjs --source ne_10m_admin_0_countries_chn.geojson \
 *     --modules . --out <repo>/web/src/viz/worldmap/world.json
 *
 * Other options: `--keep` (the share of the source's vertices that is kept, 0.08), `--min-area`
 * (the smallest ring that is drawn, in square units of the map, 0.06).
 *
 * The file is `{ width, height, scale, x, y, countries, borders }`. The map is `width` units wide
 * and `height` high; `scale`, `x` and `y` are those of Natural Earth's projection, for putting a
 * place on it. A country is `[code, name, path, area]`: its two-letter code (empty for a piece of
 * land that has none), its name in English, its outline and the area it covers in square units.
 * Paths are in tenths of a unit, `M x y l dx dy …  z`, with each step from the point before it and
 * the numbers run together where a minus sign parts them: draw them under `scale(0.1)`.
 */
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import process from 'node:process';

const args = Object.fromEntries(
  process.argv
    .slice(2)
    .join(' ')
    .split(/\s*--/)
    .filter(Boolean)
    .map((part) => {
      const [key, ...rest] = part.trim().split(/\s+/);

      return [key, rest.join(' ') || 'true'];
    }),
);

const source = args.source;
const modules = args.modules;
const out = args.out ?? 'world.json';
const KEEP = Number(args.keep ?? 0.08);
const MIN_AREA = Number(args['min-area'] ?? 0.06);
// The map is a thousand units wide, with a margin of eight all round.
const WIDTH = 1000;
const INSET = 8;

if (!source || !modules) {
  process.stderr.write('usage: build-world-map.mjs --source <geojson> --modules <dir> [--keep 0.08] [--min-area 0.06] [--out world.json]\n');
  process.exit(1);
}

const require = createRequire(path.join(path.resolve(modules), 'noop.js'));
const { topology } = require('topojson-server');
const { presimplify, simplify, quantile, sphericalTriangleArea } = require('topojson-simplify');
const { feature, mesh } = require('topojson-client');
const { geoNaturalEarth1, geoPath } = require('d3-geo');

const collection = JSON.parse(fs.readFileSync(source, 'utf8'));

// ---------------------------------------------------------------------------------------------
// Pieces: what the map draws and colours. A country is told by its two-letter code, where it has one.

/** Taiwan and its islands, as the boxes (west, east, south, north) that hold the middle of each of their pieces. */
const TAIWAN = [
  [120.0, 122.1, 21.8, 25.4],
  [119.3, 119.8, 23.1, 23.9],
];

const middleOf = (polygon) => {
  const ring = polygon[0];
  const xs = ring.map((point) => point[0]);
  const ys = ring.map((point) => point[1]);

  return [(Math.min(...xs) + Math.max(...xs)) / 2, (Math.min(...ys) + Math.max(...ys)) / 2];
};

const inTaiwan = (polygon) => {
  const [x, y] = middleOf(polygon);

  return TAIWAN.some(([west, east, south, north]) => x >= west && x <= east && y >= south && y <= north);
};

const pieces = new Map();

function add(code, name, polygons) {
  const key = code ?? `name:${name}`;
  const piece = pieces.get(key) ?? { code, name, polygons: [] };

  piece.polygons.push(...polygons);
  pieces.set(key, piece);
}

for (const item of collection.features) {
  const props = item.properties;
  const code = props.ISO_A2_EH && props.ISO_A2_EH !== '-99' ? props.ISO_A2_EH : null;

  // Nobody comes from the ice.
  if (code === 'AQ') continue;

  const polygons = item.geometry.type === 'Polygon' ? [item.geometry.coordinates] : item.geometry.coordinates;

  if (code === 'CN') {
    // The source draws Taiwan as a part of China, in the one shape. It is cut out again so that its
    // rows can be coloured; no border is drawn about it, as it shares none with the mainland.
    add('TW', 'Taiwan', polygons.filter(inTaiwan));
    add('CN', props.NAME, polygons.filter((polygon) => !inTaiwan(polygon)));
    continue;
  }

  // The pieces of one code are one country: France with Clipperton Island, Australia with its islands.
  add(code, props.NAME, polygons);
}

const features = [...pieces.values()].map((piece) => ({
  type: 'Feature',
  properties: { code: piece.code, name: piece.name },
  geometry: { type: 'MultiPolygon', coordinates: piece.polygons },
}));

// ---------------------------------------------------------------------------------------------
// Simplify the shapes together, so that two countries keep the border they share.

let topo = topology({ countries: { type: 'FeatureCollection', features } }, 1e6);
const verticesBefore = topo.arcs.reduce((total, arc) => total + arc.length, 0);

topo = presimplify(topo, sphericalTriangleArea);
topo = simplify(topo, quantile(topo, KEEP));

const verticesAfter = topo.arcs.reduce((total, arc) => total + arc.length, 0);
const shapes = feature(topo, topo.objects.countries).features;
const land = { type: 'FeatureCollection', features: shapes };

// ---------------------------------------------------------------------------------------------
// Project: Natural Earth's projection, fitted to the width of the land, with a margin all round.

const projection = geoNaturalEarth1().fitWidth(WIDTH - 2 * INSET, land);
const [translateX, translateY] = projection.translate();

projection.translate([translateX + INSET, translateY + INSET]);

const bounds = geoPath(projection).bounds(land);
const HEIGHT = Math.ceil(bounds[1][1] + INSET);

/** The rings of a shape, in the units of the map. */
function ringsOf(geometry) {
  const rings = [];
  let current = null;
  const context = {
    moveTo(x, y) {
      current = [[x, y]];
      rings.push(current);
    },
    lineTo(x, y) {
      current.push([x, y]);
    },
    closePath() {},
    arc() {},
    rect() {},
  };

  geoPath(projection, context)(geometry);

  return rings;
}

const areaOf = (ring) => {
  let total = 0;

  for (let index = 0; index < ring.length; index += 1) {
    const [x0, y0] = ring[index];
    const [x1, y1] = ring[(index + 1) % ring.length];

    total += x0 * y1 - x1 * y0;
  }

  return Math.abs(total) / 2;
};

/**
 * Path data in tenths of a unit: `M` and the first point, then every other point as the step from
 * the one before in one `l`, the numbers run together where a minus sign parts them, and `z` for a
 * ring. A ring too small to be seen is left out.
 */
function encode(rings, closed) {
  let data = '';
  let points = 0;
  let area = 0;

  for (const ring of rings) {
    const rounded = [];

    for (const [x, y] of ring) {
      const point = [Math.round(x * 10), Math.round(y * 10)];
      const last = rounded.at(-1);

      if (!last || last[0] !== point[0] || last[1] !== point[1]) rounded.push(point);
    }

    // A ring closes on its first point: it is said once.
    if (closed && rounded.length > 1 && rounded[0][0] === rounded.at(-1)[0] && rounded[0][1] === rounded.at(-1)[1]) rounded.pop();
    if (rounded.length < (closed ? 3 : 2)) continue;

    // Square tenths of a unit are a hundredth of a square unit.
    const size = closed ? areaOf(rounded) / 100 : 0;

    if (closed && size < MIN_AREA) continue;

    let steps = '';

    for (let index = 1; index < rounded.length; index += 1) {
      for (const delta of [rounded[index][0] - rounded[index - 1][0], rounded[index][1] - rounded[index - 1][1]]) {
        const text = String(delta);

        steps += steps === '' || text.startsWith('-') ? text : ` ${text}`;
      }
    }

    data += `M${rounded[0][0]} ${rounded[0][1]}l${steps}${closed ? 'z' : ''}`;
    points += rounded.length;
    area += size;
  }

  return { data, points, area };
}

const countries = [];
let drawn = 0;

for (const shape of shapes) {
  const { data, points, area } = encode(ringsOf(shape), true);

  if (data === '') continue;

  countries.push([shape.properties.code ?? '', shape.properties.name, data, Math.round(area * 100) / 100]);
  drawn += points;
}

// Not by locale: the file is the same wherever it is built.
countries.sort((a, b) => (a[1] < b[1] ? -1 : a[1] > b[1] ? 1 : 0));

const borders = encode(ringsOf(mesh(topo, topo.objects.countries, (a, b) => a !== b)), false);
const [x, y] = projection.translate();
const round = (value) => Math.round(value * 1e6) / 1e6;

// One country to a line, so that a change in the data reads as one in a diff.
const text = [
  '{',
  `  "width": ${WIDTH},`,
  `  "height": ${HEIGHT},`,
  `  "scale": ${round(projection.scale())},`,
  `  "x": ${round(x)},`,
  `  "y": ${round(y)},`,
  '  "countries": [',
  countries.map((country) => `    ${JSON.stringify(country)}`).join(',\n'),
  '  ],',
  `  "borders": ${JSON.stringify(borders.data)}`,
  '}',
  '',
].join('\n');

fs.writeFileSync(out, text);
process.stdout.write(
  `${JSON.stringify({
    keep: KEEP,
    minArea: MIN_AREA,
    vertices: { source: verticesBefore, simplified: verticesAfter, drawn, borders: borders.points },
    countries: countries.length,
    height: HEIGHT,
    bytes: text.length,
  })}\n`,
);
