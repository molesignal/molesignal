import { geoNaturalEarth1 } from 'd3-geo';
import { describe, expect, it } from 'vitest';

import { centreOf } from './centres';
import { contains, distanceTo, outlineOf } from './outline';
import { MAP_HEIGHT, MAP_WIDTH } from './size';
import { BORDERS_PATH, COUNTRIES, COUNTRY_BY_CODE, countryAt, FITTED_HEIGHT, GRATICULE_PATH, project } from './world';
import data from './world.json';

describe('the countries of the world map', () => {
  it('leave out the ice, and a piece of land without a code is a small one', () => {
    expect(COUNTRIES.find((country) => country.name === 'Antarctica')).toBeUndefined();
    expect(COUNTRIES.length).toBeGreaterThan(200);

    const nameless = COUNTRIES.filter((country) => country.code === null);

    expect(nameless.length).toBeGreaterThan(0);
    expect(nameless.every((country) => country.area < 3)).toBe(true);
  });

  it('are told by a code that is one only once', () => {
    const codes = COUNTRIES.flatMap((country) => (country.code ? [country.code] : []));

    expect(codes.every((code) => /^[A-Z]{2}$/.test(code))).toBe(true);
    expect(new Set(codes).size).toBe(codes.length);
    expect(COUNTRY_BY_CODE.get('KR')?.name).toBe('Republic of Korea');
    expect(COUNTRY_BY_CODE.get('US')?.name).toBe('United States');
    expect(COUNTRY_BY_CODE.get('FR')?.name).toBe('France');
  });

  it('are the countries their codes name: a code is where its country lies', () => {
    const wrong: string[] = [];

    for (const country of COUNTRIES) {
      const centre = country.code ? centreOf(country.code) : null;
      const place = centre ? project(centre.longitude, centre.latitude) : null;

      // A country of specks has lost most of them to the simplifying, and its marker is placed by the table alone.
      if (!country.code || !place || country.area < 1) continue;

      // The table is of about the middle of a country, and the shapes are simplified: a few hundred
      // kilometres either way, as a country of islands has its middle at sea.
      const outline = outlineOf(country.path);
      const [x, y] = [place[0] * 10, place[1] * 10];

      if (!contains(outline, x, y) && distanceTo(outline, x, y) > 150) wrong.push(`${country.code} ${country.name}`);
    }

    expect(wrong).toEqual([]);
  });

  it('have no shape for the smallest places, which are marked on the place they lie', () => {
    for (const code of ['MO', 'MV', 'MC', 'XK']) {
      expect(COUNTRY_BY_CODE.has(code)).toBe(false);
      expect(centreOf(code)).not.toBeNull();
    }

    for (const code of ['SG', 'HK', 'MT', 'BH']) {
      expect(COUNTRY_BY_CODE.get(code)?.area).toBeLessThan(1);
      expect(centreOf(code)).not.toBeNull();
    }
  });

  it('are drawn as China draws them: Arunachal Pradesh and Aksai Chin are China, Taiwan is a part of it, Kosovo is Serbia', () => {
    const at = (latitude: number, longitude: number) => countryAt(longitude, latitude)?.code;

    expect(at(27.08, 93.62)).toBe('CN'); // Itanagar
    expect(at(27.59, 91.86)).toBe('CN'); // Tawang
    expect(at(35.3, 79.8)).toBe('CN'); // Aksai Chin
    expect(at(28.6, 77.2)).toBe('IN'); // New Delhi
    expect(at(27.7, 85.3)).toBe('NP'); // Kathmandu
    expect(at(42.66, 21.17)).toBe('RS'); // Pristina
    expect(at(9.56, 44.06)).toBe('SO'); // Hargeisa
    expect(at(35.17, 33.37)).toBe('CY'); // Nicosia

    // Taiwan is drawn on a piece of its own, so that its rows can be coloured, and with no border about it.
    expect(at(25.03, 121.56)).toBe('TW'); // Taipei
    expect(at(22.63, 120.3)).toBe('TW'); // Kaohsiung
    expect(COUNTRY_BY_CODE.get('TW')?.name).toBe('Taiwan, China');
    expect(at(25.5, 119.8)).toBe('CN'); // Haitan, the island of Fujian that lies across the strait
    expect(COUNTRY_BY_CODE.has('XK')).toBe(false);
  });

  it('take a place on a simplified coast for the country whose coast it is', () => {
    // Victoria Harbour lies between two shores, and the mouth of the Pearl River is Hong Kong's.
    expect(['HK', 'CN']).toContain(countryAt(114.17, 22.3)?.code);
    expect(countryAt(-21.9, 64.15)?.code).toBe('IS'); // Reykjavik
    expect(countryAt(174.76, -36.85)?.code).toBe('NZ'); // Auckland

    // Far from land, it is nowhere.
    expect(countryAt(-140, 0)).toBeNull();
    expect(countryAt(Number.NaN, 0)).toBeNull();
    // And with no room for the sea, a place off the coast is not in the country.
    expect(countryAt(-21.9, 64.15, 0)).toBeNull();
  });

  it('are drawn in a map as tall as the world is, which is the size a page holds room for', () => {
    expect(FITTED_HEIGHT).toBe(MAP_HEIGHT);
    expect(data.width).toBe(MAP_WIDTH);
    expect(BORDERS_PATH.length).toBeGreaterThan(1_000);
    expect(GRATICULE_PATH.startsWith('M')).toBe(true);
  });

  it('put a place on the map where Natural Earth puts it', () => {
    const reference = geoNaturalEarth1().scale(data.scale).translate([data.x, data.y]);

    for (let longitude = -180; longitude <= 180; longitude += 15) {
      for (let latitude = -85; latitude <= 85; latitude += 17) {
        const expected = reference([longitude, latitude]);
        const actual = project(longitude, latitude);

        expect(expected).not.toBeNull();
        expect(actual![0]).toBeCloseTo(expected![0], 6);
        expect(actual![1]).toBeCloseTo(expected![1], 6);
      }
    }

    const singapore = project(103.8, 1.4);
    const sydney = project(151.2, -33.9);
    const london = project(-0.1, 51.5);

    expect(sydney![0]).toBeGreaterThan(singapore![0]);
    expect(sydney![1]).toBeGreaterThan(singapore![1]);
    expect(london![1]).toBeLessThan(singapore![1]);
    for (const [x, y] of [singapore!, sydney!, london!]) {
      expect(x).toBeGreaterThan(0);
      expect(x).toBeLessThan(MAP_WIDTH);
      expect(y).toBeGreaterThan(0);
      expect(y).toBeLessThan(MAP_HEIGHT);
    }
  });
});
