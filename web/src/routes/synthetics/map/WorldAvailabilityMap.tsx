import { geoContains, geoGraticule10, geoNaturalEarth1, geoPath } from 'd3-geo';
import { Minus, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { feature, mesh } from 'topojson-client';
import type { GeometryCollection, Topology } from 'topojson-specification';
import countriesTopologyJson from 'world-atlas/countries-110m.json';

import type { ProbeLocation, SyntheticResult } from '@/api/synthetics';
import { IconButton } from '@/shell/chrome';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import { AvailabilityScale } from './AvailabilityScale';
import {
  availabilityColor,
  probeHealthColor,
  summarizeProbeLocationAvailability,
  type ProbeLocationAvailability,
} from './model';

const MAP_WIDTH = 1_080;
const MAP_HEIGHT = 440;
const MAP_INSET = 18;
const MIN_ZOOM = 1;
const MAX_ZOOM = 4;
const ZOOM_STEP = 0.5;

type WorldTopology = Topology<{ countries: GeometryCollection }>;
type WorldCountry = (typeof WORLD_COUNTRIES.features)[number];

interface MapRegion {
  id: string;
  name: string;
  country: WorldCountry;
  locations: ProbeLocationAvailability[];
  availability: number | undefined;
}

interface MapViewport {
  zoom: number;
  panX: number;
  panY: number;
}

interface MapHover {
  x: number;
  y: number;
  title: string;
  regionId: string | undefined;
  locations: ProbeLocationAvailability[];
}

interface DragStart {
  pointerId: number;
  clientX: number;
  clientY: number;
  panX: number;
  panY: number;
}

const WORLD_TOPOLOGY = countriesTopologyJson as unknown as WorldTopology;
const WORLD_OBJECT = WORLD_TOPOLOGY.objects.countries;
const WORLD_COUNTRIES = feature(WORLD_TOPOLOGY, WORLD_OBJECT);
const WORLD_BORDERS = mesh(WORLD_TOPOLOGY, WORLD_OBJECT, (left, right) => left !== right);
const WORLD_GRATICULE = geoGraticule10();
const MAP_PROJECTION = geoNaturalEarth1().fitExtent(
  [
    [MAP_INSET, MAP_INSET],
    [MAP_WIDTH - MAP_INSET, MAP_HEIGHT - MAP_INSET],
  ],
  WORLD_COUNTRIES,
);
const MAP_PATH = geoPath(MAP_PROJECTION);

export function WorldAvailabilityMap({
  locations,
  results = [],
}: {
  locations: ProbeLocation[];
  results?: SyntheticResult[];
}) {
  const { t } = useTranslation('synthetics');
  const containerRef = React.useRef<HTMLDivElement>(null);
  const dragRef = React.useRef<DragStart | undefined>(undefined);
  const [viewport, setViewport] = React.useState<MapViewport>({
    zoom: MIN_ZOOM,
    panX: 0,
    panY: 0,
  });
  const [dragging, setDragging] = React.useState(false);
  const [hover, setHover] = React.useState<MapHover>();
  const { mapped, unmapped } = React.useMemo(
    () => summarizeProbeLocationAvailability(locations, results),
    [locations, results],
  );
  const regions = React.useMemo(() => mapRegions(mapped), [mapped]);
  const unmappedNames = unmapped.map((location) => location.name).join(', ');
  const viewportTransform = `translate(${viewport.panX} ${viewport.panY}) translate(${MAP_WIDTH / 2} ${MAP_HEIGHT / 2}) scale(${viewport.zoom}) translate(${-MAP_WIDTH / 2} ${-MAP_HEIGHT / 2})`;

  const zoomBy = React.useCallback((delta: number) => {
    setViewport((current) => {
      const zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, current.zoom + delta));
      if (zoom === MIN_ZOOM) return { zoom, panX: 0, panY: 0 };
      return {
        zoom,
        panX: clampPan(current.panX, zoom, MAP_WIDTH),
        panY: clampPan(current.panY, zoom, MAP_HEIGHT),
      };
    });
  }, []);

  const showHover = React.useCallback(
    (
      items: ProbeLocationAvailability[],
      event: React.PointerEvent<SVGElement>,
      title: string,
      regionId?: string,
    ) => {
      const bounds = containerRef.current?.getBoundingClientRect();
      if (!bounds) return;
      const cardWidth = Math.min(244, Math.max(180, bounds.width - 16));
      const cardHeight = Math.min(220, 54 + items.length * 46);
      setHover({
        locations: items,
        title,
        regionId,
        x: clamp(
          event.clientX - bounds.left + 12,
          8,
          Math.max(8, bounds.width - cardWidth - 8),
        ),
        y: clamp(
          event.clientY - bounds.top + 12,
          8,
          Math.max(8, bounds.height - cardHeight - 8),
        ),
      });
    },
    [],
  );

  const showFocused = React.useCallback(
    (items: ProbeLocationAvailability[], title: string, regionId?: string) => {
      const bounds = containerRef.current?.getBoundingClientRect();
      if (!bounds) return;
      setHover({ locations: items, title, regionId, x: 12, y: 12 });
    },
    [],
  );

  const beginPan = (event: React.PointerEvent<SVGSVGElement>) => {
    if (viewport.zoom <= MIN_ZOOM || event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      panX: viewport.panX,
      panY: viewport.panY,
    };
    setDragging(true);
  };

  const movePan = (event: React.PointerEvent<SVGSVGElement>) => {
    const start = dragRef.current;
    const bounds = containerRef.current?.getBoundingClientRect();
    if (!start || start.pointerId !== event.pointerId || !bounds) return;
    const deltaX = ((event.clientX - start.clientX) * MAP_WIDTH) / Math.max(1, bounds.width);
    const deltaY = ((event.clientY - start.clientY) * MAP_HEIGHT) / Math.max(1, bounds.height);
    setViewport((current) => ({
      ...current,
      panX: clampPan(start.panX + deltaX, current.zoom, MAP_WIDTH),
      panY: clampPan(start.panY + deltaY, current.zoom, MAP_HEIGHT),
    }));
  };

  const endPan = (event: React.PointerEvent<SVGSVGElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragRef.current = undefined;
    setDragging(false);
  };

  return (
    <div
      ref={containerRef}
      className="relative isolate h-[310px] overflow-hidden bg-bg-2 sm:h-[360px]"
    >
      <svg
        viewBox={`0 0 ${MAP_WIDTH} ${MAP_HEIGHT}`}
        preserveAspectRatio="xMidYMid meet"
        className={`absolute inset-0 h-full w-full select-none ${dragging ? 'cursor-grabbing' : viewport.zoom > MIN_ZOOM ? 'cursor-grab' : 'cursor-default'}`}
        style={{ touchAction: viewport.zoom > MIN_ZOOM ? 'none' : 'pan-y' }}
        role="img"
        aria-label={t('overview.map_aria_label')}
        onPointerDown={beginPan}
        onPointerMove={movePan}
        onPointerUp={endPan}
        onPointerCancel={endPan}
        onPointerLeave={() => setHover(undefined)}
        onDoubleClick={(event) => {
          event.preventDefault();
          zoomBy(ZOOM_STEP);
        }}
        onWheel={(event) => {
          const delta = event.deltaY < 0 ? ZOOM_STEP : -ZOOM_STEP;
          if (delta < 0 && viewport.zoom === MIN_ZOOM) return;
          event.preventDefault();
          zoomBy(delta);
        }}
      >
        <title>{t('overview.map_aria_label')}</title>
        <g data-map-viewport transform={viewportTransform}>
          <path
            d={MAP_PATH(WORLD_GRATICULE) ?? undefined}
            fill="none"
            stroke="var(--bd-0)"
            strokeWidth={0.7}
            opacity={0.7}
            vectorEffect="non-scaling-stroke"
          />
          {regions.map((region) => {
            const active =
              hover?.regionId === region.id ||
              region.locations.some((item) =>
                hover?.locations.some((hovered) => hovered.location.id === item.location.id),
              );
            const hasLocations = region.locations.length > 0;
            return (
              <path
                key={region.id}
                d={MAP_PATH(region.country) ?? undefined}
                fill={
                  region.availability === undefined
                    ? hasLocations
                      ? 'var(--bg-4)'
                      : 'var(--bg-3)'
                    : availabilityColor(region.availability)
                }
                data-world-country
                data-world-region={region.id}
                data-world-has-locations={hasLocations ? '' : undefined}
                tabIndex={hasLocations ? 0 : undefined}
                role={hasLocations ? 'img' : undefined}
                aria-label={
                  hasLocations
                    ? `${region.name}: ${region.locations.map((item) => item.location.name).join(', ')}`
                    : undefined
                }
                className="transition-[fill,filter] duration-fast"
                style={{
                  cursor: 'help',
                  filter: active ? 'brightness(1.18)' : undefined,
                }}
                onPointerEnter={(event) =>
                  showHover(region.locations, event, region.name, region.id)
                }
                onPointerMove={(event) =>
                  showHover(region.locations, event, region.name, region.id)
                }
                onPointerLeave={() => setHover(undefined)}
                onFocus={
                  hasLocations
                    ? () => showFocused(region.locations, region.name, region.id)
                    : undefined
                }
                onBlur={hasLocations ? () => setHover(undefined) : undefined}
              />
            );
          })}
          <path
            d={MAP_PATH(WORLD_BORDERS) ?? undefined}
            fill="none"
            stroke="var(--bg-1)"
            strokeWidth={0.8}
            pointerEvents="none"
            vectorEffect="non-scaling-stroke"
          />
          {mapped.map((item) => {
            const point = MAP_PROJECTION([...item.coordinates]);
            if (!point) return null;
            const color = probeHealthColor(item.location.health);
            const active = hover?.locations.some(
              (hovered) => hovered.location.id === item.location.id,
            );
            return (
              <g
                key={item.location.id}
                data-probe-location={item.location.id}
                transform={`translate(${point[0]} ${point[1]})`}
                tabIndex={0}
                role="img"
                aria-label={`${item.location.name}: ${t(`states.${item.location.health}`)}`}
                className="cursor-help"
                onPointerEnter={(event) => showHover([item], event, item.location.name)}
                onPointerMove={(event) => showHover([item], event, item.location.name)}
                onPointerLeave={() => setHover(undefined)}
                onFocus={() => showFocused([item], item.location.name)}
                onBlur={() => setHover(undefined)}
              >
                <title>{`${item.location.name}: ${t(`states.${item.location.health}`)}`}</title>
                <circle
                  r={14 / viewport.zoom}
                  fill={color}
                  opacity={active ? 0.28 : 0.14}
                />
                <circle
                  r={5.5 / viewport.zoom}
                  fill={color}
                  stroke="var(--bg-1)"
                  strokeWidth={2.5}
                  vectorEffect="non-scaling-stroke"
                />
              </g>
            );
          })}
        </g>
      </svg>

      {unmapped.length > 0 && (
        <div
          className="text-type-micro absolute left-3 top-3 z-10 rounded border border-bd-0 bg-bg-1/95 px-2 py-1 leading-3 text-tx-2"
          title={t('overview.unlocated_names', { names: unmappedNames })}
        >
          {t('overview.unlocated_locations', { count: unmapped.length })}
        </div>
      )}

      <TooltipProvider delayDuration={250}>
        <div className="absolute right-3 top-3 z-20 overflow-hidden rounded-md border border-bd-0 bg-bg-1/95">
          <MapControl
            label={t('overview.map_zoom_in')}
            disabled={viewport.zoom >= MAX_ZOOM}
            onClick={() => zoomBy(ZOOM_STEP)}
          >
            <Plus aria-hidden className="h-4 w-4" />
          </MapControl>
          <div className="h-px bg-bd-0" />
          <MapControl
            label={t('overview.map_zoom_out')}
            disabled={viewport.zoom <= MIN_ZOOM}
            onClick={() => zoomBy(-ZOOM_STEP)}
          >
            <Minus aria-hidden className="h-4 w-4" />
          </MapControl>
        </div>
      </TooltipProvider>

      <div className="absolute bottom-3 left-3 z-10">
        <AvailabilityScale
          values={mapped.flatMap((item) =>
            item.availability === undefined ? [] : [item.availability],
          )}
          label={t('overview.map_availability_scale')}
          ariaLabel={t('overview.map_scale_aria_label')}
        />
      </div>

      {hover && <MapHoverCard hover={hover} />}
      <span className="sr-only" aria-live="polite">
        {t('overview.map_zoom_level', { level: viewport.zoom.toFixed(1) })}
      </span>
    </div>
  );
}

function MapControl({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <IconButton
          aria-label={label}
          disabled={disabled}
          onClick={onClick}
          className="h-11 w-11 rounded-none sm:h-9 sm:w-9"
        >
          {children}
        </IconButton>
      </TooltipTrigger>
      <TooltipContent side="left">{label}</TooltipContent>
    </Tooltip>
  );
}

function MapHoverCard({ hover }: { hover: MapHover }) {
  const { t } = useTranslation('synthetics');

  return (
    <div
      role="tooltip"
      data-map-tooltip
      className="pointer-events-none absolute z-30 w-[min(244px,calc(100%-16px))] rounded-md border-0 bg-[var(--floating-surface)] p-3 text-xs text-tx-1 shadow-popup"
      style={{ left: hover.x, top: hover.y }}
    >
      <div className="mb-2 font-strong text-tx-0">{hover.title}</div>
      {hover.locations.length === 0 ? (
        <div className="text-tx-3">{t('overview.map_no_samples')}</div>
      ) : (
        <div className="divide-y divide-bd-0">
        {hover.locations.map((item) => (
          <div key={item.location.id} className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 py-2 first:pt-0 last:pb-0">
            <div className="min-w-0">
              {hover.locations.length > 1 && (
                <div className="truncate font-strong text-tx-0">{item.location.name}</div>
              )}
              <div className="mt-0.5 flex items-center gap-1.5 text-tx-2">
                <i
                  aria-hidden
                  className="h-2 w-2 rounded-full"
                  style={{ background: probeHealthColor(item.location.health) }}
                />
                {t(`states.${item.location.health}`)} · {item.location.code}
              </div>
            </div>
            <div className="text-right">
              <div className="font-code font-strong text-tx-0">
                {item.availability === undefined
                  ? '—'
                  : `${Math.round(item.availability * 100)}%`}
              </div>
              <div className="mt-0.5 text-type-micro text-tx-3">
                {item.samples > 0
                  ? t('overview.map_samples', { count: item.samples })
                  : t('overview.map_no_samples')}
              </div>
            </div>
          </div>
        ))}
        </div>
      )}
    </div>
  );
}

function mapRegions(locations: ProbeLocationAvailability[]): MapRegion[] {
  return WORLD_COUNTRIES.features.map((country, index) => {
    const regionLocations = locations.filter((item) =>
      geoContains(country, [...item.coordinates]),
    );
    const availability = regionLocations
      .map((item) => item.availability)
      .filter((value): value is number => value !== undefined);
    return {
      id: String(country.id ?? index),
      name: String(
        (country.properties as { name?: string }).name ?? country.id ?? index,
      ),
      country,
      locations: regionLocations,
      availability:
        availability.length > 0
          ? availability.reduce((total, value) => total + value, 0) / availability.length
          : undefined,
    };
  });
}

function clampPan(value: number, zoom: number, dimension: number): number {
  return clamp(value, (-dimension * (zoom - 1)) / 2, (dimension * (zoom - 1)) / 2);
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
