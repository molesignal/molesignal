import { Archive, CloudDownload, Layers, Webhook, type LucideIcon } from 'lucide-react';

import type { ConnectorKind } from './kinds';

/** One mark for each way data moves: fetched, pushed in, or written out to a store or a topic. */
export const KIND_ICONS: Record<ConnectorKind, LucideIcon> = {
  aws_cloudwatch_logs: CloudDownload,
  aws_kinesis_firehose: Webhook,
  cloudflare_logpush: Webhook,
  heroku_drain: Webhook,
  s3: Archive,
  kafka: Layers,
};
