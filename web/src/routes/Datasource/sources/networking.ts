/** Service meshes, ingress and the edge. */

import { ENDPOINT, TOKEN, type Source } from './types';

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
  {
    // Alibaba Cloud's managed Prometheus delivers cloud product metrics by remote write.
    id: 'aliyun-prometheus',
    category: 'networking',
    signals: ['metrics'],
    steps: [
      {},
      {
        code: {
          lang: 'text',
          content: `Remote Write URL: ${ENDPOINT}/api/v1/prometheus/api/v1/write?stream=aliyun
Basic Auth username: molesignal
Basic Auth password: ${TOKEN}`,
        },
      },
      {
        code: {
          lang: 'text',
          content: 'count by (__name__) ({__name__=~"Aliyun.*"})',
        },
      },
    ],
  },
  {
    // Tencent Cloud Managed Service for Prometheus writes its data to another Prometheus.
    id: 'tencent-prometheus',
    category: 'networking',
    signals: ['metrics'],
    steps: [
      {},
      {
        code: {
          lang: 'text',
          content: `Prometheus address: ${ENDPOINT}/api/v1/prometheus/api/v1/write?stream=tencent_prometheus
Username: molesignal
Password: ${TOKEN}`,
        },
      },
      { code: { lang: 'text', content: 'topk(20, count by (__name__) ({__name__=~".+"}))' } },
    ],
  },
  {
    // Huawei Cloud AOM writes a Prometheus instance's data to another Prometheus through a VPC endpoint.
    id: 'huawei-prometheus',
    category: 'networking',
    signals: ['metrics'],
    steps: [
      {},
      {
        code: {
          lang: 'text',
          content: `Remote write address: ${ENDPOINT}/api/v1/prometheus/api/v1/write?stream=huawei_prometheus
Authentication: Token
Token: ${TOKEN}`,
        },
      },
      { code: { lang: 'text', content: 'topk(20, count by (__name__) ({__name__=~".+"}))' } },
    ],
  },
];
