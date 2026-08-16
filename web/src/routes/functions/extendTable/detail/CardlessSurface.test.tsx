import { render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import '@/i18n';

import type { ExtendTableSummary } from '@/api/extendTables';

import { ExtendTableListSurface } from '../index';
import { SchemaPanel, SettingsPanel } from './InformationPanels';
import { TableMetadata } from './Metadata';
import { RecordsPanel } from './RecordsPanel';

const table: ExtendTableSummary = {
  table_name: 'service_owners',
  description: 'Service ownership lookup',
  key_field: 'service',
  value_fields: [],
  row_count: 0,
  updated_at: 1_700_000_000_000_000,
  usage_locations: [],
};

describe('Extend Table cardless surfaces', () => {
  it('keeps the list on a flat page band', () => {
    const { container } = render(
      <ExtendTableListSurface>Table rows</ExtendTableListSurface>,
    );

    const surface = container.querySelector(
      '[data-extend-table-list-surface]',
    );
    expect(surface?.className).toMatch(/border-b/);
    expect(surface?.className).not.toMatch(
      /rounded-lg|shadow|bg-bg-1|border-x|border-t/,
    );
  });

  it('uses bands and whitespace instead of rounded card containers', () => {
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
      expect(container.querySelector(selector)?.className).not.toMatch(
        /rounded-lg|shadow|bg-bg-1/,
      );
    }

    const settings = container.querySelector('[data-extend-table-settings]');
    for (const child of settings?.children ?? []) {
      expect(child.className).not.toMatch(/rounded-lg|shadow/);
    }
  });
});
