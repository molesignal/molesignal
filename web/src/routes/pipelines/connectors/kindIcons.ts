import { Archive, CloudDownload, Layers, Send, Webhook, type LucideIcon } from 'lucide-react';

import type { ConnectorKind } from './kinds';

/** One mark for each way data moves: fetched, pushed in, or written out to a store, a topic or an address. */
export const KIND_ICONS: Record<ConnectorKind, LucideIcon> = {
  aws_cloudwatch_logs: CloudDownload,
  aws_kinesis_firehose: Webhook,
  cloudflare_logpush: Webhook,
  heroku_drain: Webhook,
  s3: Archive,
  kafka: Layers,
  http: Send,
  kafka_source: CloudDownload,
  s3_source: CloudDownload,
};
