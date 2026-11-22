/** The generic intakes, listed last in Custom: webhooks and streams of events. */

import { ENDPOINT, TOKEN, type Source } from './types';

export const CUSTOM_FALLBACK_SOURCES: Source[] = [
  {
    id: 'webhook',
    category: 'custom',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/intake/logs/<stream-name> \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '[{"event":"deploy","status":"ok"}]'`,
        },
      },
    ],
  },
  {
    id: 'graphql-subscription',
    category: 'custom',
    signals: ['logs'],
    steps: [{}],
  },
];
