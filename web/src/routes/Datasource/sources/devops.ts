/** Delivery pipelines: deployments and the tools that run them. */

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
];
