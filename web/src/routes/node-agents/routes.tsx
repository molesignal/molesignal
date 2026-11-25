import type { RouteObject } from 'react-router-dom';

import { FirstOpenablePage } from '@/product/moduleAccess';

import { Enrollment } from './enrollment/Enrollment';
import { NODE_AGENTS_PAGES } from './Frame';
import { Nodes } from './nodes/Nodes';
import { Tasks } from './tasks/Tasks';

export const NODE_AGENTS_ROUTES: RouteObject[] = [
  { path: 'node-agents', element: <FirstOpenablePage paths={NODE_AGENTS_PAGES} /> },
  { path: 'node-agents/nodes', element: <Nodes /> },
  { path: 'node-agents/nodes/:nodeId', element: <Nodes /> },
  { path: 'node-agents/tasks', element: <Tasks /> },
  { path: 'node-agents/tasks/:taskId', element: <Tasks /> },
  { path: 'node-agents/enrollment', element: <Enrollment /> },
];
