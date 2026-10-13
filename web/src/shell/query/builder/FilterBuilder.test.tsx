import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import * as React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { FilterBuilder } from './FilterBuilder';
import {
  type BuilderDialect,
  type BuilderField,
  type BuilderOperator,
  operatorsForKind,
} from './model';

const FIELDS: BuilderField[] = [
  { name: 'service', kind: 'text' },
  { name: 'status', kind: 'number' },
];

const CLAUSE = /^(\w+)\s*(>=|<=|!=|=|>|<|contains)\s*(.+)$/;

/** A small field-query dialect: `a = 'x' AND b >= 2 AND free text`, `??` marks a part it rejects. */
const DIALECT: BuilderDialect = {
  parse(statement) {
    const clauses = [];
    const text: string[] = [];
    const unsupported: string[] = [];
    for (const part of statement.split(/\s+AND\s+/).map((item) => item.trim()).filter(Boolean)) {
      const match = CLAUSE.exec(part);
      if (part.startsWith('??')) unsupported.push(part);
      else if (match) {
        const raw = match[3]!;
        const quoted = raw.startsWith("'");
        clauses.push({
          field: match[1]!,
          op: match[2] as BuilderOperator,
          value: quoted ? raw.slice(1, -1) : raw,
          quoted,
        });
      } else text.push(part);
    }
    return { clauses, text: text.join(' AND '), unsupported };
  },
  serialize({ clauses, text, unsupported }) {
    return [
      ...clauses.map(
        (clause) =>
          `${clause.field} ${clause.op} ${clause.quoted === false ? clause.value : `'${clause.value}'`}`,
      ),
      ...unsupported,
      text.trim(),
    ]
      .filter(Boolean)
      .join(' AND ');
  },
  operators: (field) => operatorsForKind(field?.kind ?? 'text'),
};

/** The builder with the statement held the way a page holds it. */
function Harness({
  initial = '',
  onRun,
  pendingField,
  notice,
}: {
  initial?: string;
  onRun?: (statement: string) => void;
  pendingField?: string | null;
  notice?: string;
}) {
  const [statement, setStatement] = React.useState(initial);
  const [pending, setPending] = React.useState(pendingField ?? null);
  return (
    <>
      <output data-testid="statement">{statement}</output>
      <FilterBuilder
        statement={statement}
        onStatementChange={setStatement}
        dialect={DIALECT}
        fields={FIELDS}
        suggestValues={(field) => (field === 'status' ? ['200', '500'] : [])}
        pendingField={pending}
        onPendingFieldHandled={() => setPending(null)}
        onRun={() => onRun?.(statement)}
        notice={notice}
      />
    </>
  );
}

function statement() {
  return screen.getByTestId('statement').textContent;
}

function chips() {
  return Array.from(document.querySelectorAll('[data-query-filter]')).map((chip) =>
    chip.textContent?.replace(/\s+/g, ' ').trim(),
  );
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  // cmdk scrolls the selected item into view; jsdom has no layout to scroll.
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  cleanup();
});

describe('FilterBuilder', () => {
  it('shows the statement as filters and search text', () => {
    render(<Harness initial="service = 'checkout' AND status >= 500 AND timeout" />);

    expect(chips()).toEqual(['service=checkout', 'status>=500']);
    expect((screen.getByRole('textbox', { name: 'Search text' }) as HTMLInputElement).value).toBe(
      'timeout',
    );
  });

  it('removes a filter from the statement', () => {
    render(<Harness initial="service = 'checkout' AND status >= 500" />);

    fireEvent.click(screen.getByRole('button', { name: 'Remove filter service = checkout' }));

    expect(statement()).toBe('status >= 500');
    expect(chips()).toEqual(['status>=500']);
  });

  it('adds a filter: field, comparison, value', () => {
    render(<Harness initial="service = 'checkout'" />);

    fireEvent.click(screen.getByRole('button', { name: 'Add a filter' }));
    fireEvent.click(screen.getByRole('option', { name: /status/ }));
    // A number takes the comparisons a number has.
    const comparisons = within(screen.getByRole('group', { name: 'Comparison' }));
    expect(comparisons.queryByRole('button', { name: 'contains' })).toBeNull();
    fireEvent.click(comparisons.getByRole('button', { name: '>=' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: '500' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add filter' }));

    // The number is written bare, the way it would be typed.
    expect(statement()).toBe("service = 'checkout' AND status >= 500");
    expect(screen.queryByRole('textbox', { name: 'Value' })).toBeNull();
  });

  it('adds a filter from a value seen in the results', () => {
    render(<Harness />);

    fireEvent.click(screen.getByRole('button', { name: 'Add a filter' }));
    fireEvent.click(screen.getByRole('option', { name: /status/ }));
    fireEvent.click(screen.getByRole('button', { name: '500' }));

    expect(statement()).toBe('status = 500');
  });

  it('changes a filter that exists', () => {
    render(<Harness initial="service = 'checkout' AND status >= 500" />);

    fireEvent.click(screen.getByRole('button', { name: 'Edit filter service = checkout' }));
    const value = screen.getByRole('textbox', { name: 'Value' }) as HTMLInputElement;
    expect(value.value).toBe('checkout');
    fireEvent.click(
      within(screen.getByRole('group', { name: 'Comparison' })).getByRole('button', { name: '!=' }),
    );
    fireEvent.change(value, { target: { value: 'payments' } });
    fireEvent.click(screen.getByRole('button', { name: 'Update' }));

    expect(statement()).toBe("service != 'payments' AND status >= 500");
  });

  it('starts a filter on a field picked elsewhere', () => {
    render(<Harness pendingField="service" />);

    // The editor opens past the field list, on the field that was picked.
    expect(screen.getByRole('button', { name: 'Change field' }).textContent).toContain('service');
    fireEvent.change(screen.getByRole('textbox', { name: 'Value' }), { target: { value: 'gateway' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add filter' }));

    expect(statement()).toBe("service = 'gateway'");
  });

  it('keeps search text out of the statement until it is committed', () => {
    render(<Harness initial="service = 'checkout'" />);
    const text = screen.getByRole('textbox', { name: 'Search text' });

    fireEvent.change(text, { target: { value: 'status = 5' } });
    // Half a comparison is not a filter yet.
    expect(statement()).toBe("service = 'checkout'");

    fireEvent.change(text, { target: { value: 'disk full' } });
    fireEvent.blur(text);
    expect(statement()).toBe("service = 'checkout' AND disk full");
  });

  it('turns text typed as a comparison into a filter when it is committed', () => {
    render(<Harness />);
    const text = screen.getByRole('textbox', { name: 'Search text' }) as HTMLInputElement;

    fireEvent.change(text, { target: { value: 'status >= 500' } });
    fireEvent.blur(text);

    expect(chips()).toEqual(['status>=500']);
    expect(text.value).toBe('');
  });

  it('runs on Enter with the statement the text box just completed', () => {
    const onRun = vi.fn();
    render(<Harness initial="service = 'checkout'" onRun={onRun} />);
    const text = screen.getByRole('textbox', { name: 'Search text' });

    fireEvent.change(text, { target: { value: 'timeout' } });
    fireEvent.keyDown(text, { key: 'Enter' });

    expect(onRun).toHaveBeenCalledTimes(1);
    expect(onRun).toHaveBeenCalledWith("service = 'checkout' AND timeout");

    // Enter again changes nothing and still runs.
    fireEvent.keyDown(text, { key: 'Enter' });
    expect(onRun).toHaveBeenCalledTimes(2);
  });

  it('takes back the last filter on Backspace in an empty text box', () => {
    render(<Harness initial="service = 'checkout' AND status >= 500" />);

    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Search text' }), { key: 'Backspace' });

    expect(statement()).toBe("service = 'checkout'");
  });

  it('keeps a part it cannot show as a filter, and lets it be removed', () => {
    render(<Harness initial="service = 'checkout' AND ??weird" />);

    expect(document.querySelector('[data-query-filter-unsupported]')?.textContent).toContain('??weird');
    // Adding a filter does not drop it.
    fireEvent.click(screen.getByRole('button', { name: 'Remove filter service = checkout' }));
    expect(statement()).toBe('??weird');

    fireEvent.click(screen.getByRole('button', { name: 'Remove ??weird' }));
    expect(statement()).toBe('');
  });

  it('says so when there is something to know about the filters', () => {
    render(<Harness notice="The statement in Code is SQL." />);

    expect(screen.getByText('The statement in Code is SQL.')).not.toBeNull();
  });
});
