import { chinaPartName } from './names';
import { contains, distanceTo, type Outline, outlineOf } from './outline';
import data from './world.json';

/** One country of the map: where it is drawn, and what it is called. */
export interface WorldCountry {
  /** The two-letter code, for a piece of land that has one. */
  code: string | null;
  /** What Natural Earth calls it, in English; Taiwan, Hong Kong and Macao are called as parts of China. */
  name: string;
  /** The outline, as SVG path data in tenths of a unit of the map: it is drawn under `SHAPE_TRANSFORM`. */
  path: string;
  /** The room it takes on the map, in square units. */
  area: number;
}

/** What `scripts/build-world-map.mjs` writes to `world.json`, where it also says how. */
interface WorldData {
  height: number;
  scale: number;
  x: number;
  y: number;
  countries: [code: string, name: string, path: string, area: number][];
  borders: string;
}

const WORLD = data as unknown as WorldData;

/** The paths of the countries and of their borders are in tenths of a unit of the map. */
export const SHAPE_TRANSFORM = 'scale(0.1)';

/** The height the shapes are fitted to: the bottom of the land, and the margin the map has all round. It is `MAP_HEIGHT`. */
export const FITTED_HEIGHT = WORLD.height;

/**
 * Every country but Antarctica, with its outline: Natural Earth's, in the worldview of China,
 * where Taiwan, Hong Kong and Macao are parts of China. Taiwan is a shape of its own, with no
 * border about it, so that its rows can be coloured apart from the rest of the country.
 */
export const COUNTRIES: readonly WorldCountry[] = WORLD.countries.map(([code, name, path, area]) => ({
  code: code === '' ? null : code,
  name: chinaPartName(code, 'en') ?? name,
  path,
  area,
}));

/** The countries that have a code, by it. */
export const COUNTRY_BY_CODE: ReadonlyMap<string, WorldCountry> = new Map(
  COUNTRIES.flatMap((country) => (country.code ? [[country.code, country] as const] : [])),
);

/** The borders between countries, as one outline in tenths of a unit. */
export const BORDERS_PATH: string = WORLD.borders;

const RADIANS = Math.PI / 180;

/** Where a place lies on the map, in its units; none for a place that is not a pair of numbers. Natural Earth's projection. */
export function project(longitude: number, latitude: number): [x: number, y: number] | null {
  if (!Number.isFinite(longitude) || !Number.isFinite(latitude)) return null;

  const lambda = longitude * RADIANS;
  const phi = latitude * RADIANS;
  const phi2 = phi * phi;
  const phi4 = phi2 * phi2;
  const x = lambda * (0.8707 - 0.131979 * phi2 + phi4 * (-0.013791 + phi4 * (0.003971 * phi2 - 0.001529 * phi4)));
  const y = phi * (1.007226 + phi2 * (0.015085 + phi4 * (-0.044475 + 0.028874 * phi2 - 0.005916 * phi4)));

  return [WORLD.x + WORLD.scale * x, WORLD.y - WORLD.scale * y];
}

const decimal = (value: number) => Math.round(value * 10) / 10;

/** The lines of longitude and latitude, every ten degrees, in the units of the map. */
function graticule(): string {
  let path = '';

  // A meridian bends: it is drawn from a point every five degrees. Those of a quarter turn run to the poles.
  for (let longitude = -180; longitude <= 180; longitude += 10) {
    const reach = longitude % 90 === 0 ? 90 : 80;

    for (let latitude = -reach; latitude <= reach; latitude += 5) {
      const point = project(longitude, latitude);

      if (point) path += `${latitude === -reach ? 'M' : 'L'}${decimal(point[0])} ${decimal(point[1])}`;
    }
  }

  // A parallel is straight.
  for (let latitude = -80; latitude <= 80; latitude += 10) {
    const west = project(-180, latitude);
    const east = project(180, latitude);

    if (west && east) path += `M${decimal(west[0])} ${decimal(west[1])}L${decimal(east[0])} ${decimal(east[1])}`;
  }

  return path;
}

export const GRATICULE_PATH: string = graticule();

/** How far from the coast of a country a place may lie and still be taken for its. It is in units of the map. */
const COAST_REACH = 3;

const outlines = new Map<WorldCountry, Outline>();

const outlineFor = (country: WorldCountry): Outline => {
  const known = outlines.get(country);

  if (known) return known;

  const outline = outlineOf(country.path);

  outlines.set(country, outline);

  return outline;
};

/**
 * The country a place is in. The shapes are simplified, so a place on a coast can lie a little
 * out to sea: one within `reach` units of the map of a country's coast is taken to be in it, the
 * nearest if there are several. None for a place far from any land.
 */
export function countryAt(longitude: number, latitude: number, reach = COAST_REACH): WorldCountry | null {
  const place = project(longitude, latitude);

  if (!place) return null;

  // The outlines are in tenths of a unit.
  const x = place[0] * 10;
  const y = place[1] * 10;
  const slack = reach * 10;
  let nearest: WorldCountry | null = null;
  let nearestDistance = slack;

  for (const country of COUNTRIES) {
    const outline = outlineFor(country);

    if (x < outline.minX - slack || x > outline.maxX + slack || y < outline.minY - slack || y > outline.maxY + slack) continue;
    if (contains(outline, x, y)) return country;

    const distance = distanceTo(outline, x, y);

    if (distance <= nearestDistance) {
      nearest = country;
      nearestDistance = distance;
    }
  }

  return nearest;
}
