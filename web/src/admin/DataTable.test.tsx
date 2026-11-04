import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import '@/i18n';

import { DataTable, type DataTableColumn } from './DataTable';

afterEach(cleanup);

interface Row {
  id: string;
  name: string;
  size: number | null;
}

const ROWS: Row[] = [
  { id: 'a', name: 'alpha', size: 2 },
  { id: 'b', name: 'beta', size: null },
  { id: 'c', name: 'gamma', size: 10 },
];

const COLUMNS: DataTableColumn<Row>[] = [
  { key: 'name', header: 'Name', value: (row) => row.name, cell: (row) => row.name },
  { key: 'size', header: 'Size', value: (row) => row.size, sortable: true, cell: (row) => row.size ?? '–' },
];

function Picking({ rows = ROWS, initial = [] }: { rows?: Row[]; initial?: string[] }) {
  const [selected, setSelected] = React.useState<ReadonlySet<string>>(new Set(initial));

  return (
    <>
      <output data-testid="picked">{[...selected].sort().join(',')}</output>
      <DataTable
        rows={rows}
        columns={COLUMNS}
        rowKey={(row) => row.id}
        selection={{ selected, onChange: setSelected, isSelectable: (row) => row.id !== 'b' }}
      />
    </>
  );
}

function shown(): string[] {
  return screen.getAllByRole('row').slice(1).map((row) => row.textContent ?? '');
}

describe('DataTable', () => {
  it('leaves out of the page what is meant for a file', () => {
    const columns = [...COLUMNS, { key: 'file', header: 'In the file', fileOnly: true, value: () => 1, cell: () => 1 }];
    render(<DataTable rows={ROWS} columns={columns} rowKey={(row) => row.id} />);

    expect(screen.queryByRole('columnheader', { name: 'In the file' })).toBeNull();
    expect(screen.getAllByRole('columnheader')).toHaveLength(2);
  });

  it('holds its columns where asked to, without stretching an empty list', () => {
    const { unmount } = render(
      <DataTable rows={ROWS} columns={COLUMNS} rowKey={(row) => row.id} fixedMinWidth={900} />,
    );
    const table = screen.getByRole('table');

    expect(table.className).toContain('table-fixed');
    expect(table.style.minWidth).toBe('900px');
    unmount();

    render(<DataTable rows={[]} columns={COLUMNS} rowKey={(row) => row.id} fixedMinWidth={900} />);
    expect(screen.queryByRole('table')).toBeNull();
  });

  it('keeps the head and draws bars while the rows are on their way', () => {
    render(<DataTable rows={[]} columns={COLUMNS} rowKey={(row) => row.id} loading />);

    expect(screen.getByRole('columnheader', { name: 'Name' })).not.toBeNull();
    expect(screen.getByRole('table').getAttribute('aria-busy')).toBe('true');
    expect(screen.queryByText('No rows')).toBeNull();
  });

  it('sorts up, down and back, with what has no value last', () => {
    render(<DataTable rows={ROWS} columns={COLUMNS} rowKey={(row) => row.id} />);
    const size = screen.getByRole('button', { name: 'Size' });
    const head = screen.getByRole('columnheader', { name: 'Size' });

    fireEvent.click(size);
    expect(shown()).toEqual(['alpha2', 'gamma10', 'beta–']);
    expect(head.getAttribute('aria-sort')).toBe('ascending');

    fireEvent.click(size);
    expect(shown()).toEqual(['gamma10', 'alpha2', 'beta–']);
    expect(head.getAttribute('aria-sort')).toBe('descending');

    fireEvent.click(size);
    expect(shown()).toEqual(['alpha2', 'beta–', 'gamma10']);
    expect(head.getAttribute('aria-sort')).toBeNull();
  });

  it('ticks only what can be ticked, and offers no box on the rest', () => {
    render(<Picking />);

    expect(screen.getAllByRole('checkbox', { name: 'Select row' })).toHaveLength(2);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Select all rows' }));
    expect(screen.getByTestId('picked').textContent).toBe('a,c');
    expect(screen.getByRole('checkbox', { name: 'Select all rows' }).getAttribute('aria-checked')).toBe('true');
  });

  it('leaves what is ticked on another page alone when ticking all of this one', () => {
    render(<Picking initial={['elsewhere']} />);
    const all = screen.getByRole('checkbox', { name: 'Select all rows' });

    fireEvent.click(all);
    expect(screen.getByTestId('picked').textContent).toBe('a,c,elsewhere');

    fireEvent.click(all);
    expect(screen.getByTestId('picked').textContent).toBe('elsewhere');
  });

  it('opens a row by a click or a key on it, and leaves a box in it to the box', () => {
    const onOpen = vi.fn();
    const onChange = vi.fn();
    render(
      <DataTable
        rows={ROWS}
        columns={COLUMNS}
        rowKey={(row) => row.id}
        onRowClick={onOpen}
        selection={{ selected: new Set(), onChange }}
      />,
    );
    const [box] = screen.getAllByRole('checkbox', { name: 'Select row' });
    const row = screen.getByRole('row', { name: /alpha/ });

    fireEvent.keyDown(box as HTMLElement, { key: ' ' });
    fireEvent.click(box as HTMLElement);
    expect(onOpen).not.toHaveBeenCalled();
    expect(onChange).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(row, { key: 'Enter' });
    expect(onOpen).toHaveBeenCalledTimes(1);
  });
});
