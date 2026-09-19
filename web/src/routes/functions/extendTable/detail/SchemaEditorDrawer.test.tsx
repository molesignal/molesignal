import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { ExtendTableSummary, ExtendValueField } from '@/api/extendTables';
import i18n from '@/i18n';

import { SchemaEditorDrawer } from './SchemaEditorDrawer';

afterEach(cleanup);

const field: ExtendValueField = {
  name: 'tier',
  field_type: 'string',
  required: false,
  description: i18n.t('functions:extend_tables.field_description'),
};

function table(rowCount: number): ExtendTableSummary {
  return {
    table_name: 'customers',
    description: i18n.t('functions:extend_tables.definition_description'),
    key_field: 'customer_id',
    value_fields: [field],
    row_count: rowCount,
    updated_at: 1_700_000_000_000_000,
    usage_locations: [],
  };
}

function renderDrawer(
  rowCount: number,
  onSave: (fields: ExtendValueField[]) => void = vi.fn(),
) {
  return render(
    <SchemaEditorDrawer
      access={{ allowed: true, disabled: false }}
      open
      table={table(rowCount)}
      fields={[field]}
      busy={false}
      onClose={vi.fn()}
      onSave={onSave}
    />,
  );
}

describe('SchemaEditorDrawer', () => {
  it('locks destructive changes for existing fields when records exist', async () => {
    const user = userEvent.setup();
    renderDrawer(3);

    expect(screen.getByLabelText('Field 1 name')).toBeDisabled();
    expect(screen.getByLabelText('Field 1 type')).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Remove field tier' })).toBeDisabled();
    expect(screen.getByLabelText('Field 1 description')).toBeEnabled();
    expect(screen.getByRole('checkbox', { name: 'Require field tier' })).toBeEnabled();

    await user.click(screen.getByRole('button', { name: 'Add value field' }));

    expect(screen.getByLabelText('Field 2 name')).toBeEnabled();
    expect(screen.getByLabelText('Field 2 type')).toBeEnabled();
    expect(
      screen.getByRole('button', { name: 'Remove field Field 2' }),
    ).toBeEnabled();
  });

  it('allows destructive schema changes while the table is empty', async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    renderDrawer(0, onSave);

    const nameInput = screen.getByLabelText('Field 1 name');
    expect(nameInput).toBeEnabled();
    expect(screen.getByLabelText('Field 1 type')).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Remove field tier' })).toBeEnabled();

    await user.clear(nameInput);
    await user.type(nameInput, 'plan');
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(onSave).toHaveBeenCalledWith([
      {
        ...field,
        name: 'plan',
      },
    ]);
  });
});
