import { act, cleanup, render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { useReportRenderState, type ReportRenderState } from './readiness';

function Page({ state, error }: { state: ReportRenderState; error?: unknown }) {
  useReportRenderState(state, error);
  return null;
}

/** A stand-in for `document.fonts`, which jsdom does not have. */
function installFonts(status: 'loading' | 'loaded') {
  const fonts = Object.assign(new EventTarget(), { status }) as EventTarget & {
    status: 'loading' | 'loaded';
  };
  Object.defineProperty(document, 'fonts', { configurable: true, value: fonts });

  return {
    settle(next: 'loading' | 'loaded') {
      fonts.status = next;
      fonts.dispatchEvent(new Event(next === 'loaded' ? 'loadingdone' : 'loading'));
    },
  };
}

const root = document.documentElement;

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(document, 'fonts');
});

describe('useReportRenderState', () => {
  it('publishes where the page stands on the document element', () => {
    const view = render(<Page state="loading" />);
    expect(root.dataset.reportRenderState).toBe('loading');

    view.rerender(<Page state="ready" />);
    expect(root.dataset.reportRenderState).toBe('ready');
    expect(root.dataset.reportRenderError).toBeUndefined();
  });

  it('says why a report cannot be rendered, and takes it back when it can', () => {
    const view = render(<Page state="error" error={new Error('dashboard not found')} />);
    expect(root.dataset.reportRenderState).toBe('error');
    expect(root.dataset.reportRenderError).toBe('dashboard not found');

    view.rerender(<Page state="error" error="no access" />);
    expect(root.dataset.reportRenderError).toBe('no access');

    view.rerender(<Page state="error" />);
    expect(root.dataset.reportRenderError).toBe('report could not be rendered');

    view.rerender(<Page state="loading" />);
    expect(root.dataset.reportRenderState).toBe('loading');
    expect(root.dataset.reportRenderError).toBeUndefined();
  });

  it('leaves nothing on the document once the page is gone', () => {
    const view = render(<Page state="error" error="gone" />);
    view.unmount();

    expect(root.dataset.reportRenderState).toBeUndefined();
    expect(root.dataset.reportRenderError).toBeUndefined();
  });

  it('holds ready back until the fonts are in', () => {
    const fonts = installFonts('loading');
    render(<Page state="ready" />);
    expect(root.dataset.reportRenderState).toBe('loading');

    act(() => fonts.settle('loaded'));
    expect(root.dataset.reportRenderState).toBe('ready');

    // A face first used by late content starts loading after the page was ready.
    act(() => fonts.settle('loading'));
    expect(root.dataset.reportRenderState).toBe('loading');

    act(() => fonts.settle('loaded'));
    expect(root.dataset.reportRenderState).toBe('ready');
  });

  it('does not hold an error back for fonts', () => {
    installFonts('loading');
    render(<Page state="error" error="boom" />);

    expect(root.dataset.reportRenderState).toBe('error');
    expect(root.dataset.reportRenderError).toBe('boom');
  });
});
