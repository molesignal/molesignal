/** Web servers and proxies on a host. */

import { ENDPOINT_HOST, ENDPOINT_PORT, ENDPOINT_TLS, TOKEN, type Source } from './types';

export const SERVER_SOURCES: Source[] = [
  {
    id: 'nginx',
    category: 'servers',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'ini',
          content: `[INPUT]
    Name        tail
    Path        /var/log/nginx/access.log
    Parser      nginx
    Tag         nginx.access

[OUTPUT]
    Name        http
    Match       nginx.*
    Host        ${ENDPOINT_HOST}
    Port        ${ENDPOINT_PORT}
    URI         /api/v1/intake/logs/default
    Format      json
    Header      Authorization Bearer ${TOKEN}
    tls         ${ENDPOINT_TLS}`,
        },
      },
    ],
  },
  {
    id: 'apache',
    category: 'servers',
    signals: ['logs'],
    steps: [
      {},
    ],
  },
  {
    id: 'haproxy',
    category: 'servers',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'text',
          content: `global
  log 127.0.0.1:6514 local0
  log-format '{"client":"%ci","status":%ST,"path":"%HU","resp_ms":%Tr}'`,
        },
      },
    ],
  },
];
