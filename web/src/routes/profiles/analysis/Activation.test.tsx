import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { ProfileActivation } from './Activation';

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(() => {
  cleanup();
});

describe('ProfileActivation', () => {
  it('offers both ways to get a first profile in', () => {
    const onUpload = vi.fn();
    render(
      <MemoryRouter>
        <ProfileActivation onUpload={onUpload} />
      </MemoryRouter>,
    );

    expect(
      screen.getByRole('heading', { level: 2, name: 'No profiles in this window' }),
    ).not.toBeNull();
    expect(screen.getByRole('link', { name: 'Set up OTLP / Pyroscope' }).getAttribute('href')).toBe(
      '/datasource/recommended/continuous-profiling',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Upload' }));
    expect(onUpload).toHaveBeenCalledTimes(1);
  });

  it('does not offer an upload the user may not make', () => {
    render(
      <MemoryRouter>
        <ProfileActivation />
      </MemoryRouter>,
    );

    expect(screen.queryByRole('button', { name: 'Upload' })).toBeNull();
    expect(screen.getByRole('link', { name: 'Set up OTLP / Pyroscope' })).not.toBeNull();
  });
});
