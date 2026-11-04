import { Activity, BellRing, ChartSpline, Cloud, Dog, Mail, Siren, Webhook, type LucideIcon } from 'lucide-react';

import type { AlertSourceKind } from '@/api/alertSources';

/** The sign of each kind of sender, set before a source's name. */
export const SOURCE_KIND_ICONS: Record<AlertSourceKind, LucideIcon> = {
  api: Webhook,
  alertmanager: BellRing,
  grafana: ChartSpline,
  datadog: Dog,
  pagerduty: Siren,
  aliyun_cms: Cloud,
  tencent_monitor: Cloud,
  aws_cloudwatch: Cloud,
  zabbix: Activity,
  email: Mail,
};
