import { Archive, CloudDownload, Layers, Send, Webhook, type LucideIcon } from 'lucide-react';

import type { ConnectorKind } from './kinds';

/** One mark for each way data moves: fetched, pushed in, or written out to a store, a topic or an address. */
export const KIND_ICONS: Record<ConnectorKind, LucideIcon> = {
  aws_cloudwatch_logs: CloudDownload,
  aws_kinesis_firehose: Webhook,
  gcp_pubsub_push: Webhook,
  cloudflare_logpush: Webhook,
  heroku_drain: Webhook,
  s3: Archive,
  kafka: Layers,
  http: Send,
  kafka_source: CloudDownload,
  s3_source: CloudDownload,
  gcp_pubsub_pull: CloudDownload,
  aws_cloudwatch_metrics: CloudDownload,
  azure_monitor_metrics: CloudDownload,
  gcp_monitoring_metrics: CloudDownload,
  aliyun_cms_metrics: CloudDownload,
  tencent_monitor_metrics: CloudDownload,
  huawei_ces_metrics: CloudDownload,
  cloud_events: Webhook,
};
