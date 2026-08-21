import { render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ExtendTableSummary } from '@/api/extendTables';
import i18n from '@/i18n';

import { ExtendTableListSurface } from '../index';
import { SchemaPanel, SettingsPanel } from './InformationPanels';
import { TableMetadata } from './Metadata';
import { RecordsPanel } from './RecordsPanel';

const table: ExtendTableSummary = {
  table_name: 'service_owners',
  description: i18n.t('functions:extend_tables.definition_description'),
  key_field: 'service',
  value_fields: [],
  row_count: 0,
  updated_at: 1_700_000_000_000_000,
  usage_locations: [],
};

describe('Extend Table surface hierarchy', () => {
  it('keeps the list borderless inside the shared list surface', () => {
    const { container } = render(
      <ExtendTableListSurface>
        {i18n.t('functions:extend_tables.tabs.records')}
      </ExtendTableListSurface>,
    );

    const surface = container.querySelector(
      '[data-extend-table-list-surface]',
    );
    expect(surface?.className).toContain('bg-transparent');
    expect(surface?.className).not.toMatch(/border|shadow/);
  });

  it('uses borderless functional surfaces for detail regions', () => {
    const { container } = render(
      <>
        <TableMetadata table={table} />
        <RecordsPanel
          editAccess={{ allowed: true, disabled: false }}
          table={table}
          rows={[]}
          allRows={[]}
          fields={[]}
          totalFieldCount={0}
          search=""
          onSearchChange={vi.fn()}
          expandedKey={null}
          onExpandedKeyChange={vi.fn()}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onAdd={vi.fn()}
          onImport={vi.fn()}
        />
        <SettingsPanel
          deleteAccess={{ allowed: true, disabled: false }}
          table={table}
          onDelete={vi.fn()}
        />
        <SchemaPanel
          editAccess={{ allowed: true, disabled: false }}
          table={table}
          fields={[]}
          onEdit={vi.fn()}
        />
      </>,
    );

    for (const selector of [
      '[data-extend-table-metadata]',
      '[data-extend-table-records]',
      '[data-extend-table-settings]',
      '[data-extend-table-schema]',
    ]) {
      const region = container.querySelector(selector);
      if (selector !== '[data-extend-table-metadata]') {
        expect(region?.className).toContain('rounded-md');
        expect(region?.className).toContain('shadow-functional-surface');
      }
      expect(region?.className).not.toMatch(/\bborder/);
    }

    const settings = container.querySelector('[data-extend-table-settings]');
    for (const child of settings?.children ?? []) {
      expect(child.className).toContain('rounded-md');
      expect(child.className).not.toMatch(/\bborder/);
    }
  });
});
