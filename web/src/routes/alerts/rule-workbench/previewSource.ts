import { apmApi } from '@/api/apm';
import * as rumApi from '@/api/rum';
import type { QueryResult } from '@/types/query';

import { apmNewErrorsPreview, apmPreviewResult } from './apmSource';
import { fitted, rumPreviewResult } from './rumSource';
import type { TypedSource } from './typedSource';

const HOUR_MICROS = 3_600_000_000;

/** What a rule on a figure of another product would have seen lately, as the rows of a query. */
export interface SourcePreview {
  result: QueryResult;
  /** The stretch of time the rows are of, in microseconds. */
  from: number;
  to: number;
  /** Why there is no figure, when the product could not say. */
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked';
}

/**
 * Reads the figure over the last six hours. APM's pages carry a trend per bucket and the figure is
 * worked out from it; RUM's preview is worked out by the server, by the code that evaluates the
 * rule, window by window.
 */
export async function previewSource(source: TypedSource, periodSecs: number): Promise<SourcePreview> {
  const started = Date.now();

  if (source.kind === 'rum') {
    const preview = await rumApi.previewAlert({ query: fitted(source.query), periodSecs: periodSecs });

    return {
      result: rumPreviewResult(preview, Date.now() - started),
      from: preview.from,
      to: preview.to,
      ...(preview.unavailable ? { unavailable: preview.unavailable } : {}),
    };
  }

  const draft = source.query;
  const to = Date.now() * 1_000;
  const from = to - 6 * HOUR_MICROS;
  const scope = {
    from,
    to,
    ...(draft.namespace ? { namespace: draft.namespace } : {}),
    ...(draft.environment ? { environment: draft.environment } : {}),
  };
  const detail = draft.transaction
    ? await apmApi.transaction(draft.transaction.name, { ...scope, service: draft.service, kind: draft.transaction.kind })
    : await apmApi.service(draft.service, scope);
  const bucketSeconds = detail.meta.resolution === 'hour' ? 3_600 : 60;
  const result =
    draft.metric === 'new_error_groups'
      ? apmNewErrorsPreview(detail.trend, detail.errors, bucketSeconds, Date.now() - started)
      : apmPreviewResult(detail.trend, draft.metric, bucketSeconds, Date.now() - started);

  return { result, from, to };
}
