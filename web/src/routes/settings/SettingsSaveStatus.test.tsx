import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';

import {
  SettingsSaveStatusIndicator,
  SettingsSaveStatusProvider,
  useSettingsSaveStatus,
} from './SettingsSaveStatus';

let controls: ReturnType<typeof useSettingsSaveStatus>;

function Harness() {
  controls = useSettingsSaveStatus();
  return <SettingsSaveStatusIndicator />;
}

describe('SettingsSaveStatusIndicator', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(cleanup);

  it('opens on a quiet check: nothing has just landed, so nothing pops', () => {
    const { container } = render(
      <SettingsSaveStatusProvider>
        <Harness />
      </SettingsSaveStatusProvider>,
    );

    expect(screen.getByRole('status').textContent).toBe('All changes saved');
    expect(container.querySelector('.ms-check-in')).toBeNull();
  });

  it('runs the product pulse while saving, then lets the check arrive', () => {
    const { container } = render(
      <SettingsSaveStatusProvider>
        <Harness />
      </SettingsSaveStatusProvider>,
    );

    act(() => controls.beginSave());
    expect(screen.getByRole('status').textContent).toBe('Saving…');
    expect(container.querySelector('path.ms-pulse-travel')).not.toBeNull();
    expect(container.querySelector('.animate-spin')).toBeNull();

    act(() => controls.completeSave());
    expect(screen.getByRole('status').textContent).toBe('All changes saved');
    expect(container.querySelector('path.ms-pulse-travel')).toBeNull();
    expect(container.querySelector('svg.ms-check-in')).not.toBeNull();
  });

  it('flags a failed save', () => {
    render(
      <SettingsSaveStatusProvider>
        <Harness />
      </SettingsSaveStatusProvider>,
    );

    act(() => controls.beginSave());
    act(() => controls.failSave());
    expect(screen.getByRole('status').textContent).toBe('Save failed. Try again');
  });
});
