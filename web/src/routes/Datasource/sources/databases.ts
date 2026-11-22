/** Databases and caches. */

import type { Source } from './types';

export const DATABASE_SOURCES: Source[] = [
  {
    id: 'postgres',
    category: 'databases',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'sql',
          content: `ALTER SYSTEM SET log_min_duration_statement = 500;
ALTER SYSTEM SET log_statement = 'ddl';
SELECT pg_reload_conf();`,
        },
      },
      {},
    ],
  },
  {
    id: 'mysql',
    category: 'databases',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'ini',
          content: `[mysqld]
slow_query_log = 1
slow_query_log_file = /var/log/mysql/slow.log
long_query_time = 0.5`,
        },
      },
    ],
  },
  {
    id: 'mongodb',
    category: 'databases',
    signals: ['logs', 'metrics'],
    steps: [
      { code: { lang: 'yaml', content: `systemLog:\n  destination: file\n  path: /var/log/mongo/mongod.log\n  logAppend: true` } },
    ],
  },
  {
    id: 'clickhouse',
    category: 'databases',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'sql',
          content: `SELECT * FROM system.query_log
WHERE event_time >= now() - INTERVAL 1 MINUTE
FORMAT JSONEachRow`,
        },
      },
    ],
  },
  {
    id: 'redis',
    category: 'databases',
    signals: ['metrics', 'logs'],
    steps: [
      { code: { lang: 'bash', content: 'docker run -d -p 9121:9121 oliver006/redis_exporter -redis.addr=redis://localhost:6379' } },
      {},
    ],
  },
];
