import {
  cleanup,
  fireEvent,
  render,
  screen,
} from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import type { ActivationState } from '@/product/activation';

import { QuickStartDrawer } from './QuickStartDrawer';

const state: ActivationState = {
  completedCount: 1,
  totalCount: 2,
  ready: false,
  steps: [
    {
      id: 'datasource',
      labelKey: 'steps.datasource.title',
      descriptionKey: 'steps.datasource.description',
      completed: true,
      to: '/datasource',
    },
    {
      id: 'sample-data',
      labelKey: 'steps.sample_data.title',
      descriptionKey: 'steps.sample_data.description',
      completed: false,
      to: '/datasource/recommended/http-json',
    },
  ],
};

afterEach(cleanup);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('QuickStartDrawer', () => {
  it('renders quick start outside the home layout and routes each step', () => {
    const onOpenStep = vi.fn();
    const onLoadSample = vi.fn();

    render(
      <QuickStartDrawer
        open
        onOpenChange={vi.fn()}
        state={state}
        onOpenStep={onOpenStep}
        onLoadSample={onLoadSample}
        loadingSample={false}
      />,
    );

    expect(
      screen.getByRole('dialog', { name: 'Quick start' }),
    ).not.toBeNull();
    expect(
      screen.getByText(
        'Complete the essential setup steps for this workspace.',
      ),
    ).not.toBeNull();

    fireEvent.click(
      screen.getByRole('button', { name: /Connect a datasource/ }),
    );
    fireEvent.click(
      screen.getByRole('button', { name: /Load sample data/ }),
    );

    expect(onOpenStep).toHaveBeenCalledWith('/datasource');
    expect(onLoadSample).toHaveBeenCalledOnce();
  });
});
