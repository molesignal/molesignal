/**
 * Datasource catalogue.
 *
 * What does not translate: each source's identifier, the signals it sends and
 * the commands of its guide. Its name, description and the words of each step
 * are in the `datasources` i18n namespace under the source's id, one entry per
 * step in the order of `steps` here (see `sourceText.ts`).
 *
 * Every access snippet below targets the REAL backend intake contract
 * (`bin/molesignal/src/api/http/routes/*`): native JSON on `/api/v1/intake/{signal}/{stream}`,
 * OTLP/HTTP on `/api/v1/{logs,metrics,traces}`, OTLP gRPC on `:4317`,
 * ES bulk on `/api/v1/_bulk`, Prometheus on `/api/v1/prometheus/api/v1/write`,
 * push connectors on `/api/v1/_kinesis_firehose|_cloudflare|_heroku`. Auth is
 * `Authorization: Bearer <token>` everywhere except push connectors (which use
 * a connector token). The org is derived server-side from the token and never
 * appears in a path.
 */

import { AI_SOURCES } from './ai';
import { CUSTOM_SOURCES } from './custom';
import { CUSTOM_FALLBACK_SOURCES } from './customFallback';
import { DATABASE_SOURCES } from './databases';
import { DEVOPS_SOURCES } from './devops';
import { LANGUAGE_SOURCES } from './languages';
import { NETWORKING_SOURCES } from './networking';
import { QUEUE_SOURCES } from './queues';
import { RECOMMENDED_SOURCES } from './recommended';
import { SECURITY_SOURCES } from './security';
import { SERVER_SOURCES } from './servers';
import type { Category, Source } from './types';

export type { Category, GuideStep, Signal, Source } from './types';

/** Every source, in the order the catalogue lists them. */
export const SOURCES: Source[] = [
  ...RECOMMENDED_SOURCES,
  ...CUSTOM_SOURCES,
  ...SERVER_SOURCES,
  ...DATABASE_SOURCES,
  ...SECURITY_SOURCES,
  ...DEVOPS_SOURCES,
  ...NETWORKING_SOURCES,
  ...QUEUE_SOURCES,
  ...LANGUAGE_SOURCES,
  ...AI_SOURCES,
  ...CUSTOM_FALLBACK_SOURCES,
];

export function sourcesIn(cat: Category): Source[] {
  return SOURCES.filter((s) => s.category === cat);
}
