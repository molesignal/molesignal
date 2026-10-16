import { describe, expect, it } from 'vitest';

import type { ComponentStatus, StatusPageComponent } from '@/api/statusPages';

import { componentRows, componentSiblings } from './rows';

function component(
  id: string,
  position: number,
  overrides: Partial<StatusPageComponent> = {},
): StatusPageComponent {
  return {
    id,
    org_id: 'org',
    status_page_id: 'page',
    name: id,
    description: '',
    status: 'operational' as ComponentStatus,
    visibility: 'enabled',
    lifecycle: 'active',
    position,
    archived_at: null,
    created_at: 1,
    updated_at: 1,
    kind: 'component',
    group_id: null,
    ...overrides,
  };
}

const components = [
  component('api', 10),
  component('ai', 20, { kind: 'group', name: 'AI SRE' }),
  component('chat', 30, { group_id: 'ai', name: 'Web Chat' }),
  component('sandbox', 40, { group_id: 'ai', name: 'Sandbox', status: 'partial_outage' }),
  component('billing', 50),
];

describe('component rows', () => {
  it('lists each group with its components under it, the group in their worst status', () => {
    const rows = componentRows(components, { lifecycle: 'active', search: '' });

    expect(rows.map((row) => [row.component.id, row.depth])).toEqual([
      ['api', 0],
      ['ai', 0],
      ['chat', 1],
      ['sandbox', 1],
      ['billing', 0],
    ]);
    expect(rows[1]?.status).toBe('partial_outage');
  });

  it('keeps a group while one of its components matches a filter', () => {
    const rows = componentRows(components, { lifecycle: 'active', search: '', status: 'partial_outage' });
    expect(rows.map((row) => row.component.id)).toEqual(['ai', 'sandbox']);
  });

  it('keeps every component of a group found by its own name', () => {
    const rows = componentRows(components, { lifecycle: 'active', search: 'sre' });
    expect(rows.map((row) => row.component.id)).toEqual(['ai', 'chat', 'sandbox']);
  });

  it('moves a component among the others of its group, and a group among the top level', () => {
    expect(componentSiblings(components, components[2]!).map((item) => item.id)).toEqual(['chat', 'sandbox']);
    expect(componentSiblings(components, components[1]!).map((item) => item.id)).toEqual(['api', 'ai', 'billing']);
  });
});
