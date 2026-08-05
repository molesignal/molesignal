import '@/i18n';

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { afterEach, describe, expect, it } from 'vitest';

import { VariableQueryEditor } from './VariableQueryEditor';

afterEach(cleanup);

describe('VariableQueryEditor', () => {
  it('replaces raw JSON with a label-values selection and structured fields', () => {
    render(<Harness initialValue={{}} />);

    expect(
      (screen.getByRole('combobox', {
        name: 'Query type',
      }) as HTMLSelectElement).value,
    ).toBe('label_values');
    expect(screen.queryByDisplayValue('{}')).toBeNull();

    fireEvent.change(screen.getByRole('textbox', { name: 'Metric' }), {
      target: { value: 'http_requests_total' },
    });
    fireEvent.change(screen.getByRole('textbox', { name: 'Label' }), {
      target: { value: 'service' },
    });

    expect(currentValue()).toMatchObject({
      kind: 'query',
      queryType: 'label_values',
      metric: 'http_requests_total',
      label: 'service',
      expression: 'label_values(http_requests_total, service)',
    });
  });

  it('infers structured fields from a legacy label_values expression', () => {
    render(
      <Harness
        initialValue={{
          expression: 'label_values(http_requests_total, instance)',
        }}
      />,
    );

    expect(
      (screen.getByRole('combobox', {
        name: 'Query type',
      }) as HTMLSelectElement).value,
    ).toBe('label_values');
    expect(
      (screen.getByRole('textbox', { name: 'Metric' }) as HTMLInputElement)
        .value,
    ).toBe('http_requests_total');
    expect(
      (screen.getByRole('textbox', { name: 'Label' }) as HTMLInputElement)
        .value,
    ).toBe('instance');
  });

  it('preserves classic queries and exposes SQL stream selections', () => {
    render(<Harness initialValue={{ expression: 'custom_variable_query()' }} />);

    const queryType = screen.getByRole('combobox', {
      name: 'Query type',
    }) as HTMLSelectElement;
    expect(queryType.value).toBe('classic');
    expect(
      (screen.getByRole('textbox', { name: 'Query' }) as HTMLTextAreaElement)
        .value,
    ).toBe('custom_variable_query()');

    fireEvent.change(queryType, { target: { value: 'sql' } });
    fireEvent.change(screen.getByRole('textbox', { name: 'Stream name' }), {
      target: { value: 'otel_metrics' },
    });
    fireEvent.change(screen.getByRole('combobox', { name: 'Stream type' }), {
      target: { value: 'metrics' },
    });
    fireEvent.change(screen.getByRole('textbox', { name: 'SQL query' }), {
      target: { value: 'SELECT DISTINCT service FROM otel_metrics' },
    });

    expect(currentValue()).toMatchObject({
      kind: 'sql',
      queryType: 'sql',
      streamName: 'otel_metrics',
      streamType: 'metrics',
      expression: 'SELECT DISTINCT service FROM otel_metrics',
    });
  });
});

function Harness({
  initialValue,
}: {
  initialValue: Record<string, unknown>;
}) {
  const [value, setValue] = React.useState(initialValue);
  return (
    <>
      <VariableQueryEditor value={value} onChange={setValue} />
      <output data-testid="query-value">{JSON.stringify(value)}</output>
    </>
  );
}

function currentValue(): Record<string, unknown> {
  return JSON.parse(screen.getByTestId('query-value').textContent ?? '{}') as Record<
    string,
    unknown
  >;
}
