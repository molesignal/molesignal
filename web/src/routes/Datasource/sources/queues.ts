/** Message queues and brokers. */

import { ENDPOINT, TOKEN, type Source } from './types';

export const QUEUE_SOURCES: Source[] = [
  {
    id: 'kafka',
    category: 'queues',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'json',
          content: `{
  "name": "molesignal-sink",
  "config": {
    "connector.class": "io.confluent.connect.http.HttpSinkConnector",
    "topics": "app-events",
    "http.api.url": "${ENDPOINT}/api/v1/intake/logs/default",
    "headers": "Authorization:Bearer ${TOKEN}",
    "request.method": "POST"
  }
}`,
        },
      },
    ],
  },
  {
    id: 'rabbitmq',
    category: 'queues',
    signals: ['metrics', 'logs'],
    steps: [{ code: { lang: 'bash', content: 'rabbitmq-plugins enable rabbitmq_prometheus' } }],
  },
  {
    id: 'nats',
    category: 'queues',
    signals: ['metrics', 'logs'],
    steps: [{}],
  },
];
