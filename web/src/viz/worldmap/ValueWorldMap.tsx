import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { centreOf } from './centres';
import { type MapControlLabels, MapControls } from './MapControls';
import { MAP_HEIGHT, MAP_WIDTH } from './size';
import { useMapViewport } from './useMapViewport';
import { BORDERS_PATH, COUNTRIES, GRATICULE_PATH, project, SHAPE_TRANSFORM, type WorldCountry } from './world';

/** A country and the number it is coloured by, with the words that tell it. */
export interface MapValue {
  /** The two-letter code of the country. */
  code: string;
  value: number;
  /** What the country is called. */
  name: string;
  /** The number as it is told. */
  text: string;
}

export interface ValueWorldMapLabels extends MapControlLabels {
  /** The zoom, said to a reader who cannot see the map. */
  zoomLevel: (zoom: number) => string;
  /** What a country with no number is told as. */
  noData: string;
  /** What the keys do, for a reader who has the map in focus. */
  keys: string;
  /** How to zoom a map that leaves the plain wheel to the page. */
  wheelHint: string;
}

export interface MapLegend {
  /** What the colour is of. */
  label: string;
  low: string;
  high: string;
}

const SIZE = { width: MAP_WIDTH, height: MAP_HEIGHT };

/** How long the way to zoom a map is said for, after the wheel was left to the page. */
const HINT_MS = 1_400;

/** The room, in square units of the map, below which a country is too small to be seen: it is marked on the place it lies. */
const SEEN_AREA = 6;

const isSeen = (country: WorldCountry) => country.area >= SEEN_AREA;

/** What tells a country from the others: a few shapes have no code. */
const countryKey = (country: WorldCountry) => `${country.code ?? ''}:${country.name}`;

/** The colour of a country at `share` (0 to 1) of the largest number: the land, tinted more the larger it is. */
const rampColor = (share: number) => `color-mix(in srgb, var(--indigo) ${Math.round(18 + 82 * share)}%, var(--bg-3))`;

/** A square root, so that a country with a few is not lost beside one with thousands. */
const shareOf = (value: number, largest: number) => (largest > 0 ? Math.sqrt(Math.max(0, value) / largest) : 0);

interface Hover {
  /** What the pointer is on: a country by its code and name, a marker by its code. */
  key: string;
  name: string;
  text: string | null;
  x: number;
  y: number;
}

/**
 * The world, country by country, in the colour of each country's number: the countries of the
 * data are tinted by how large it is, and a country too small to have a shape of its own is a
 * marker on the place it lies. It zooms with the buttons, the wheel, a pinch and the keys, and
 * moves by dragging; the pointer on a country says what it is and what it came to.
 */
export function ValueWorldMap({
  values,
  ariaLabel,
  labels,
  legend,
  nameOf,
  wheelZoom = 'always',
  className,
}: {
  values: readonly MapValue[];
  ariaLabel: string;
  labels: ValueWorldMapLabels;
  legend?: MapLegend | undefined;
  /** What a country is called in the language of the page, by its code and its English name. */
  nameOf: (code: string | null, english: string) => string;
  /** `modifier` leaves the plain wheel to the page, for a map set in the middle of one that scrolls. */
  wheelZoom?: 'always' | 'modifier';
  className?: string;
}) {
  const [hinted, setHinted] = React.useState(false);
  const hintTimer = React.useRef<ReturnType<typeof setTimeout>>();
  const showHint = React.useCallback(() => {
    setHinted(true);
    clearTimeout(hintTimer.current);
    hintTimer.current = setTimeout(() => setHinted(false), HINT_MS);
  }, []);
  React.useEffect(() => () => clearTimeout(hintTimer.current), []);

  const map = useMapViewport(SIZE, { wheel: wheelZoom, onWheelHint: showHint });
  const containerRef = React.useRef<HTMLDivElement>(null);
  const keysId = React.useId();
  const [hover, setHover] = React.useState<Hover | null>(null);

  const byCode = React.useMemo(() => new Map(values.map((entry) => [entry.code.toUpperCase(), entry])), [values]);
  const largest = React.useMemo(() => Math.max(0, ...values.map((entry) => entry.value)), [values]);
  const seen = React.useMemo(
    () => new Set(COUNTRIES.flatMap((country) => (country.code && isSeen(country) ? [country.code] : []))),
    [],
  );

  // The countries of the data that have no shape on this map, or none to be seen: marked where they lie.
  const markers = React.useMemo(
    () =>
      values.flatMap((entry) => {
        const code = entry.code.toUpperCase();
        const centre = seen.has(code) ? null : centreOf(code);
        const place = centre ? project(centre.longitude, centre.latitude) : null;

        return place ? [{ entry, code, x: place[0], y: place[1] }] : [];
      }),
    [seen, values],
  );
  const listed = React.useMemo(() => [...values].sort((a, b) => b.value - a.value), [values]);

  const show = React.useCallback(
    (key: string, name: string, text: string | null, event: React.PointerEvent<Element>) => {
      const bounds = containerRef.current?.getBoundingClientRect();

      if (!bounds) return;

      const width = Math.min(220, Math.max(160, bounds.width - 16));

      setHover({
        key,
        name,
        text,
        x: Math.min(Math.max(event.clientX - bounds.left + 12, 8), Math.max(8, bounds.width - width - 8)),
        y: Math.min(Math.max(event.clientY - bounds.top + 12, 8), Math.max(8, bounds.height - 64)),
      });
    },
    [],
  );
  const hide = React.useCallback(() => setHover(null), []);

  const onCountry = React.useCallback(
    (country: WorldCountry, event: React.PointerEvent<Element>) => {
      const entry = country.code ? byCode.get(country.code) : undefined;

      show(countryKey(country), nameOf(country.code, country.name), entry?.text ?? null, event);
    },
    [byCode, nameOf, show],
  );

  const grabbing = map.dragging ? 'cursor-grabbing' : map.viewport.zoom > 1 ? 'cursor-grab' : 'cursor-default';

  return (
    <div
      ref={containerRef}
      role="group"
      aria-label={ariaLabel}
      aria-describedby={keysId}
      tabIndex={0}
      data-world-map=""
      data-map-zoom={map.viewport.zoom.toFixed(2)}
      onKeyDown={map.onKeyDown}
      className={cn('group relative isolate w-full overflow-hidden rounded-md bg-bg-2', className)}
      style={{ aspectRatio: `${MAP_WIDTH} / ${MAP_HEIGHT}` }}
    >
      <svg
        ref={map.svgRef}
        viewBox={`0 0 ${MAP_WIDTH} ${MAP_HEIGHT}`}
        preserveAspectRatio="xMidYMid meet"
        aria-hidden="true"
        className={cn('absolute inset-0 h-full w-full select-none', grabbing)}
        style={{ touchAction: map.viewport.zoom > 1 ? 'none' : 'pan-y' }}
        onPointerLeave={hide}
        {...map.svgHandlers}
      >
        <g data-map-viewport="" transform={map.transform}>
          <path d={GRATICULE_PATH} fill="none" stroke="var(--bd-0)" strokeWidth={0.7} opacity={0.7} vectorEffect="non-scaling-stroke" />
          <g transform={SHAPE_TRANSFORM}>
            <Countries byCode={byCode} largest={largest} active={map.dragging ? null : (hover?.key ?? null)} onEnter={onCountry} onLeave={hide} />
            <path d={BORDERS_PATH} fill="none" stroke="var(--bg-1)" strokeWidth={0.8} pointerEvents="none" vectorEffect="non-scaling-stroke" />
            <Outlines byCode={byCode} />
          </g>
          {markers.map((marker) => {
            const color = rampColor(shareOf(marker.entry.value, largest));
            const radius = (3.5 + 3 * shareOf(marker.entry.value, largest)) / map.viewport.zoom;

            return (
              <g
                key={marker.code}
                data-map-country={marker.code}
                data-map-marker=""
                transform={`translate(${marker.x} ${marker.y})`}
                className="cursor-help"
                onPointerEnter={(event) => show(marker.code, marker.entry.name, marker.entry.text, event)}
                onPointerMove={(event) => show(marker.code, marker.entry.name, marker.entry.text, event)}
                onPointerLeave={hide}
              >
                <circle r={radius * 2.4} fill="var(--indigo)" opacity={0.2} />
                <circle r={radius} fill={color} stroke="var(--indigo-soft)" strokeWidth={1} vectorEffect="non-scaling-stroke" />
              </g>
            );
          })}
        </g>
      </svg>

      <div className="absolute right-3 top-3 z-20">
        <MapControls
          labels={labels}
          canZoomIn={map.canZoomIn}
          canZoomOut={map.canZoomOut}
          onZoomIn={map.zoomIn}
          onZoomOut={map.zoomOut}
          onReset={map.reset}
        />
      </div>

      {legend && (
        <div
          data-map-legend=""
          className="absolute bottom-3 left-3 z-10 w-[min(11rem,45%)] rounded-md border border-bd-0 bg-bg-1/95 px-2.5 py-1.5 font-sans"
        >
          <div className="type-micro mb-1 truncate font-strong text-tx-1">{legend.label}</div>
          <div aria-hidden className="h-1.5 rounded-full" style={{ background: `linear-gradient(90deg, ${rampColor(0)}, ${rampColor(1)})` }} />
          <div className="type-micro mt-1 flex justify-between tabular-nums text-tx-3">
            <span>{legend.low}</span>
            <span>{legend.high}</span>
          </div>
        </div>
      )}

      {/* The project draws no focus frame: a map the keyboard is on says which keys it answers to. */}
      <span
        aria-hidden
        data-map-keys=""
        className="type-micro pointer-events-none absolute bottom-3 right-3 z-10 rounded-md border border-bd-0 bg-bg-1/95 px-2 py-1 font-sans tabular-nums text-tx-1 opacity-0 transition-opacity group-focus-visible:opacity-100"
      >
        + − 0 ← ↑ ↓ →
      </span>

      <div
        role="status"
        data-map-hint=""
        aria-hidden={!hinted}
        className={cn(
          'pointer-events-none absolute inset-0 z-20 grid place-items-center bg-bg-1/70 px-6 text-center font-sans text-sm font-strong text-tx-0 transition-opacity',
          hinted ? 'opacity-100' : 'opacity-0',
        )}
      >
        {hinted ? labels.wheelHint : ''}
      </div>

      {hover && !map.dragging && (
        <div
          role="tooltip"
          data-map-tooltip=""
          className="pointer-events-none absolute z-30 w-[min(220px,calc(100%-16px))] rounded-md border-0 bg-[var(--floating-surface)] p-3 font-sans text-xs text-tx-1 shadow-popup"
          style={{ left: hover.x, top: hover.y }}
        >
          <div className="font-strong text-tx-0">{hover.name}</div>
          <div className={cn('mt-0.5 tabular-nums', hover.text === null ? 'text-tx-3' : 'font-code font-strong text-tx-0')}>
            {hover.text ?? labels.noData}
          </div>
        </div>
      )}

      {/* What the map says, for a reader who does not see it. */}
      <span id={keysId} className="sr-only">
        {labels.keys}
      </span>
      <ul className="sr-only">
        {listed.map((entry) => (
          <li key={entry.code}>
            {entry.name}: {entry.text}
          </li>
        ))}
      </ul>
      <span className="sr-only" aria-live="polite">
        {labels.zoomLevel(map.viewport.zoom)}
      </span>
    </div>
  );
}

/** The land, a country at a time: tinted where the data has a number for it. Drawn again only when what it shows changes. */
const Countries = React.memo(function Countries({
  byCode,
  largest,
  active,
  onEnter,
  onLeave,
}: {
  byCode: ReadonlyMap<string, MapValue>;
  largest: number;
  /** The country the pointer is on, by its key. */
  active: string | null;
  onEnter: (country: WorldCountry, event: React.PointerEvent<Element>) => void;
  onLeave: () => void;
}) {
  return (
    <>
      {COUNTRIES.map((country) => {
        const entry = country.code ? byCode.get(country.code) : undefined;
        const hovered = active !== null && active === countryKey(country);

        return (
          <path
            key={countryKey(country)}
            d={country.path}
            fill={entry ? rampColor(shareOf(entry.value, largest)) : 'var(--bg-3)'}
            data-world-country={country.code ?? country.name}
            data-map-country={entry && isSeen(country) ? country.code : undefined}
            className="transition-[fill,filter] duration-fast"
            style={{ filter: hovered ? 'brightness(1.18)' : undefined }}
            onPointerEnter={(event) => onEnter(country, event)}
            onPointerMove={(event) => onEnter(country, event)}
            onPointerLeave={onLeave}
          />
        );
      })}
    </>
  );
});

/** The outline of each country that has a number, over the borders: a small one is a mark you can see. */
const Outlines = React.memo(function Outlines({ byCode }: { byCode: ReadonlyMap<string, MapValue> }) {
  return (
    <>
      {COUNTRIES.flatMap((country) =>
        country.code && isSeen(country) && byCode.has(country.code)
          ? [
              <path
                key={country.code}
                d={country.path}
                fill="none"
                stroke="var(--indigo-soft)"
                strokeWidth={1}
                opacity={0.75}
                pointerEvents="none"
                vectorEffect="non-scaling-stroke"
              />,
            ]
          : [],
      )}
    </>
  );
});
