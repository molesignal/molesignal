import { describe, expect, it } from 'vitest';

import type { OwnedActionItem } from '@/api/postmortems';
import type { AppNotification } from '@/shell/notifications/model';
import type { Incident } from '@/types/alerting';

import { buildTodos, type TodoSources } from './todos';

function incident(fields: Partial<Incident>): Incident {
  return { status: 'open', assignees: [], severity: 'error', createdAt: 1, ...fields } as Incident;
}

function actionItem(id: string, dueAt: number | null): OwnedActionItem {
  return { id, dueAt, title: id, postmortemTitle: 'Checkout outage', status: 'open' } as OwnedActionItem;
}

const approval: AppNotification = { id: 'approval:u1', category: 'approvals', title: 'Approve Ada' };
const alert: AppNotification = { id: 'incident:i9', category: 'alerts', title: 'Checkout p95' };

function sources(fields: Partial<TodoSources>): TodoSources {
  return {
    userId: 'me',
    nowMicros: 100,
    incidents: [],
    notifications: [],
    actionItems: [],
    streamGaps: 0,
    activation: null,
    ...fields,
  };
}

describe('buildTodos', () => {
  it('lists what waits on the reader in the order it should be done', () => {
    const todos = buildTodos(
      sources({
        incidents: [
          incident({ id: 'warn', severity: 'warning' }),
          incident({ id: 'critical', severity: 'critical' }),
          incident({ id: 'theirs', assignees: ['someone'] }),
        ],
        notifications: [alert, approval],
        actionItems: [actionItem('due', 500), actionItem('late', 50)],
        streamGaps: 2,
        activation: { remaining: 2, nextLabel: 'Notifications' },
      }),
    );

    expect(todos.map((todo) => todo.key)).toEqual([
      'incident:critical',
      'incident:warn',
      'approval:u1',
      'action:late',
      'stream-gap',
      'action:due',
      'activation',
    ]);
  });

  it('says nothing of streams or setting up when there is nothing to do', () => {
    const todos = buildTodos(sources({ streamGaps: 0, activation: { remaining: 0, nextLabel: null } }));

    expect(todos).toEqual([]);
  });

  it('marks an action item past its date as overdue', () => {
    const [late] = buildTodos(sources({ actionItems: [actionItem('late', 50)] }));

    expect(late).toMatchObject({ kind: 'action_item', overdue: true });
  });
});
