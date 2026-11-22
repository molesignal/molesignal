/** Service meshes, ingress and the edge. */

import type { Source } from './types';

export const NETWORKING_SOURCES: Source[] = [
  {
    id: 'envoy',
    category: 'networking',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        code: {
          lang: 'yaml',
          content: `access_log:
- name: envoy.access_loggers.http_grpc
  typed_config:
    '@type': type.googleapis.com/envoy.extensions.access_loggers.grpc.v3.HttpGrpcAccessLogConfig
    common_config:
      log_name: envoy
      grpc_service:
        envoy_grpc: { cluster_name: molesignal }`,
        },
      },
    ],
  },
  {
    id: 'traefik',
    category: 'networking',
    signals: ['logs', 'metrics'],
    steps: [
      { code: { lang: 'yaml', content: `accessLog:\n  format: json\n  filePath: /var/log/traefik/access.json\nmetrics:\n  prometheus: {}` } },
    ],
  },
  {
    id: 'cloudflare',
    category: 'networking',
    signals: ['logs'],
    steps: [
      {},
    ],
  },
];
