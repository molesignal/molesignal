import { describe, expect, it } from 'vitest';

import {
  changesHandsOnSave,
  rendersPage,
  resolveRunAs,
  runAsName,
} from './model';

const ME = 'u-me';

describe('rendersPage', () => {
  it('is true for the formats the headless browser prints', () => {
    expect(rendersPage('pdf')).toBe(true);
    expect(rendersPage('png')).toBe(true);
    expect(rendersPage('csv')).toBe(false);
    expect(rendersPage('json')).toBe(false);
    expect(rendersPage('svg')).toBe(false);
    expect(rendersPage(undefined)).toBe(false);
  });
});

describe('resolveRunAs', () => {
  it('names the user a saved report runs as when it is someone else', () => {
    expect(
      resolveRunAs({
        saved: { format: 'pdf', userId: 'u-dana' },
        format: 'pdf',
        currentUserId: ME,
      }),
    ).toEqual({ kind: 'other', userId: 'u-dana' });
  });

  it('recognises the person looking at the report', () => {
    expect(
      resolveRunAs({
        saved: { format: 'png', userId: ME },
        format: 'png',
        currentUserId: ME,
      }),
    ).toEqual({ kind: 'self' });
  });

  it('reports a PDF or PNG report that no signed-in user saved as failing', () => {
    expect(
      resolveRunAs({
        saved: { format: 'pdf', userId: null },
        format: 'pdf',
        currentUserId: ME,
      }),
    ).toEqual({ kind: 'missing', format: 'pdf' });

    // The deliveries that fail are those of the saved format, whatever is chosen next.
    expect(
      resolveRunAs({
        saved: { format: 'pdf', userId: null },
        format: 'png',
        currentUserId: ME,
      }),
    ).toEqual({ kind: 'missing', format: 'pdf' });
  });

  it('states nothing for a format that renders no page', () => {
    expect(
      resolveRunAs({
        saved: { format: 'csv', userId: null },
        format: 'csv',
        currentUserId: ME,
      }),
    ).toBeNull();
    expect(
      resolveRunAs({
        saved: { format: 'json', userId: 'u-dana' },
        format: 'json',
        currentUserId: ME,
      }),
    ).toBeNull();
    // A PDF report about to become CSV stops needing a user.
    expect(
      resolveRunAs({
        saved: { format: 'pdf', userId: null },
        format: 'csv',
        currentUserId: ME,
      }),
    ).toBeNull();
  });

  it('runs a report that has not been saved yet as the person about to save it', () => {
    expect(
      resolveRunAs({ saved: null, format: 'pdf', currentUserId: ME }),
    ).toEqual({ kind: 'self' });
    expect(
      resolveRunAs({ saved: null, format: 'csv', currentUserId: ME }),
    ).toBeNull();
  });

  it('does not call a report failing when it never needed a user', () => {
    // Saved as CSV without a user: nothing failed. Choosing PDF makes the saver its user.
    expect(
      resolveRunAs({
        saved: { format: 'csv', userId: null },
        format: 'pdf',
        currentUserId: ME,
      }),
    ).toEqual({ kind: 'self' });

    // The editor shows a stored SVG report as PDF. What it stored is what decides.
    expect(
      resolveRunAs({
        saved: { format: 'svg', userId: null },
        format: 'pdf',
        currentUserId: ME,
      }),
    ).toEqual({ kind: 'self' });
  });

  it('states nothing when the backend does not report the user', () => {
    expect(
      resolveRunAs({
        saved: { format: 'pdf', userId: undefined },
        format: 'pdf',
        currentUserId: ME,
      }),
    ).toBeNull();
  });
});

describe('runAsName', () => {
  it('prefers the display name, then the email, then the id', () => {
    const profile = { display_name: 'Dana Reyes', email: 'dana@example.com' };

    expect(runAsName(profile, 'u-dana')).toBe('Dana Reyes');
    expect(runAsName({ ...profile, display_name: '' }, 'u-dana')).toBe(
      'dana@example.com',
    );
    // The user record is gone, or the backend did not send it.
    expect(runAsName(null, 'u-dana')).toBe('u-dana');
    expect(runAsName(undefined, 'u-dana')).toBe('u-dana');
  });
});

describe('changesHandsOnSave', () => {
  it('is true only when saving replaces the user the report runs as', () => {
    expect(changesHandsOnSave({ kind: 'other', userId: 'u-dana' })).toBe(true);
    expect(changesHandsOnSave({ kind: 'missing', format: 'pdf' })).toBe(true);
    expect(changesHandsOnSave({ kind: 'self' })).toBe(false);
    expect(changesHandsOnSave(null)).toBe(false);
  });
});
