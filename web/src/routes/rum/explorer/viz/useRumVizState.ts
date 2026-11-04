import { useQuery, type UseQueryResult } from '@tanstack/react-query';
import * as React from 'react';

import * as rumApi from '@/api/rum';
import type { RumAggregate, RumDistribution, RumField } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { utcOffsetMicros } from '@/shell/query/histogram/axis';

import type { ParsedRumStatement } from '../fieldQuery';
import type { RumTab } from '../model';
import {
  readRumVizAddress,
  type RumViz,
  rumVizFromParams,
  type RumVizAddress,
  writeRumVizParams,
  writeRumVizSettings,
} from './address';
import {
  COUNTRY_FIELD,
  distributionRequest,
  resolveVizSpec,
  type RumVizRequest,
  type RumVizSpec,
  vizRequest,
} from './spec';

/** What the explorer needs to draw the rows as numbers: the way of drawing, what is asked, and the answer. */
export interface RumVizModel {
  /** The way of drawing in front: the list when the address names one the type cannot be drawn in. */
  viz: RumViz;
  /** Whether rows can be drawn on a map: the type has where its clients were. */
  canMap: boolean;
  /** What is asked of the rows; none while the list is in front or the fields are not known. */
  spec: RumVizSpec | null;
  request: RumVizRequest | null;
  query: UseQueryResult<RumAggregate, Error>;
  /** The answer of a distribution, which is not an aggregate. */
  distribution: UseQueryResult<RumDistribution, Error>;
  /** The range the numbers are of, in microseconds. */
  range: { from: number; to: number };
  onVizChange: (viz: RumViz) => void;
  onSettingsChange: (patch: Partial<RumVizAddress>) => void;
}

interface RumVizInputs {
  orgId: string;
  tab: RumTab;
  fields: readonly RumField[];
  /** Whether the fields of the type are known: nothing is asked before. */
  ready: boolean;
  searchParams: URLSearchParams;
  setSearchParams: (update: (current: URLSearchParams) => URLSearchParams, options: { replace: boolean }) => void;
  range: { from: number; to: number };
  parsed: ParsedRumStatement;
  /** Goes up when the reader asks for the results again. */
  epoch: number;
}

/**
 * How the rows the query matches are drawn, as the address says it: the way of drawing, the
 * numbers asked, and the answer of the server. The address holds it all, so a copy of it draws
 * the same.
 */
export function useRumVizState({
  orgId,
  tab,
  fields,
  ready,
  searchParams,
  setSearchParams,
  range,
  parsed,
  epoch,
}: RumVizInputs): RumVizModel {
  const clock = useTimeFormatter();
  const canMap = React.useMemo(() => fields.some((field) => field.name === COUNTRY_FIELD), [fields]);
  const named = rumVizFromParams(searchParams);
  // A map of a type that has no countries is the list, not a map of nothing.
  const viz = named === 'map' && ready && !canMap ? 'list' : named;

  const address = React.useMemo(() => readRumVizAddress(searchParams), [searchParams]);
  const spec = React.useMemo(
    () => (viz === 'list' || !ready ? null : resolveVizSpec(viz, address, tab, fields)),
    [address, fields, ready, tab, viz],
  );

  const request = React.useMemo(
    () =>
      spec
        ? vizRequest(spec, {
            tab,
            range,
            filters: parsed.filters,
            freeText: parsed.freeText,
            utcOffsetMicros: utcOffsetMicros(range.from, clock.tz),
          })
        : null,
    [clock.tz, parsed.filters, parsed.freeText, range, spec, tab],
  );

  const query = useQuery<RumAggregate, Error>({
    queryKey: ['rum', 'viz', orgId, viz, request?.params, epoch],
    enabled: Boolean(orgId) && request !== null && viz !== 'distribution',
    queryFn: ({ signal }) => rumApi.aggregate((request as RumVizRequest).params, signal),
    // The numbers of the way of drawing stay while the next arrive, and never stand in another's place.
    placeholderData: (previous, previousQuery) => (previousQuery?.queryKey[3] === viz ? previous : undefined),
  });

  const asked = React.useMemo(
    () =>
      spec
        ? distributionRequest(spec, {
            tab,
            range,
            filters: parsed.filters,
            freeText: parsed.freeText,
            utcOffsetMicros: 0,
          })
        : null,
    [parsed.filters, parsed.freeText, range, spec, tab],
  );

  const distribution = useQuery<RumDistribution, Error>({
    queryKey: ['rum', 'viz', orgId, 'distribution', asked, epoch],
    enabled: Boolean(orgId) && asked !== null,
    queryFn: ({ signal }) => rumApi.distribution(asked as NonNullable<typeof asked>, signal),
    placeholderData: (previous) => previous,
  });

  const onVizChange = React.useCallback(
    (next: RumViz) => setSearchParams((current) => writeRumVizParams(current, next), { replace: true }),
    [setSearchParams],
  );

  const onSettingsChange = React.useCallback(
    (patch: Partial<RumVizAddress>) => setSearchParams((current) => writeRumVizSettings(current, patch), { replace: true }),
    [setSearchParams],
  );

  return { viz, canMap, spec, request, query, distribution, range, onVizChange, onSettingsChange };
}
