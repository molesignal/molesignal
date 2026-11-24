import { ENDPOINT, TOKEN, type Source } from './types';

/**
 * Tools whose webhooks send a body of their own: they post to
 * `POST /api/v1/intake/changes`, which reads the body by its `format` and
 * takes what it leaves unsaid from the address. The token rides in the
 * address for a tool that sets no header, and needs `apm.deploy`.
 */

const DOCS = 'https://docs.molesignal.com/en-US/apm/deployments#report-changes-from-webhooks';
const INTAKE = `${ENDPOINT}/api/v1/intake/changes`;

export const CHANGE_WEBHOOK_SOURCES: Source[] = [
  {
    id: 'launchdarkly',
    category: 'devops',
    signals: ['traces'],
    docsUrl: DOCS,
    steps: [
      {
        code: {
          lang: 'text',
          content: `${INTAKE}?format=launchdarkly&service=checkout&token=${TOKEN}`,
        },
      },
      {
        code: {
          lang: 'json',
          content: `[
  {
    "effect": "allow",
    "actions": ["*"],
    "resources": ["proj/*:env/production:flag/*"]
  }
]`,
        },
      },
    ],
  },
  {
    id: 'unleash',
    category: 'devops',
    signals: ['traces'],
    docsUrl: DOCS,
    steps: [
      {
        code: {
          lang: 'text',
          content: `URL:                ${INTAKE}?format=unleash&service=checkout
Extra HTTP headers: {"Authorization": "Bearer ${TOKEN}"}
Events:             feature-environment-enabled, feature-environment-disabled, feature-strategy-updated`,
        },
      },
    ],
  },
  {
    id: 'nacos',
    category: 'devops',
    signals: ['traces'],
    docsUrl: DOCS,
    steps: [
      {
        code: {
          lang: 'text',
          content: `${INTAKE}?format=nacos&service=checkout&environment=production&token=${TOKEN}`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `curl -X POST "${INTAKE}?format=nacos&service=checkout&environment=production" \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '{"dataId": "checkout.yaml", "group": "DEFAULT_GROUP", "namespace": "prod", "srcUser": "ada"}'`,
        },
      },
    ],
  },
  {
    id: 'apollo',
    category: 'devops',
    signals: ['traces'],
    docsUrl: DOCS,
    steps: [
      {
        code: {
          lang: 'text',
          content: `${INTAKE}?format=apollo&environment=production&token=${TOKEN}`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `curl -X POST "${INTAKE}?format=apollo&environment=production" \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '{"appId": "checkout", "clusterName": "default", "namespaceName": "application", "releaseTitle": "raise pool", "operator": "ada"}'`,
        },
      },
    ],
  },
];
