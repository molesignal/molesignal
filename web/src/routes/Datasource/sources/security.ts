/** Security agents and audit trails. */

import { ENDPOINT, TOKEN, type Source } from './types';

export const SECURITY_SOURCES: Source[] = [
  {
    id: 'falco',
    category: 'security',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'yaml',
          content: `webhook:\n  address: ${ENDPOINT}/api/v1/intake/logs/security\n  customHeaders: "Authorization: Bearer ${TOKEN}"`,
        },
      },
    ],
  },
  {
    id: 'osquery',
    category: 'security',
    signals: ['logs'],
    steps: [
      { code: { lang: 'json', content: '{"options":{"logger_plugin":"http","logger_http_url":"https://...","schedule":{"users":{"query":"select * from logged_in_users;","interval":60}}}}' } },
    ],
  },
  {
    id: 'crowdstrike',
    category: 'security',
    signals: ['logs'],
    steps: [{}],
  },
];
