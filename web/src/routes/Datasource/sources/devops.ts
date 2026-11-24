/** Delivery pipelines: deployments, the tools that run them, and the tools whose changes are reported by webhook. */

import { CHANGE_WEBHOOK_SOURCES } from './changeWebhooks';
import { DEPLOYMENT_SOURCES } from './deploy';
import type { Source } from './types';

export const DEVOPS_SOURCES: Source[] = [
  ...DEPLOYMENT_SOURCES,
  {
    id: 'argocd',
    category: 'devops',
    signals: ['logs', 'metrics'],
    steps: [{}],
  },
  ...CHANGE_WEBHOOK_SOURCES,
];
