import * as React from 'react';

export type ReportRenderState = 'loading' | 'ready' | 'error';

/**
 * Tells the report renderer where the page stands.
 *
 * The renderer loads the page in a headless browser and waits on
 * `<html data-report-render-state>`: it prints at `ready`, and at `error` it
 * fails the report with `data-report-render-error` instead of printing a page
 * that says something went wrong. `ready` is held back until the fonts are in,
 * so the text is not printed in a fallback face.
 */
export function useReportRenderState(state: ReportRenderState, error?: unknown): void {
  const fontsLoaded = useFontsLoaded();
  const published: ReportRenderState = state === 'ready' && !fontsLoaded ? 'loading' : state;
  const message = published === 'error' ? errorMessage(error) : null;

  React.useEffect(() => {
    const root = document.documentElement;
    root.dataset.reportRenderState = published;
    if (message === null) delete root.dataset.reportRenderError;
    else root.dataset.reportRenderError = message;

    return () => {
      delete root.dataset.reportRenderState;
      delete root.dataset.reportRenderError;
    };
  }, [message, published]);
}

function useFontsLoaded(): boolean {
  const [loaded, setLoaded] = React.useState(() => fontSet()?.status !== 'loading');

  React.useEffect(() => {
    const fonts = fontSet();
    if (!fonts) return;

    // A face is asked for when text first uses it, so loading can begin after the page is up.
    // `loadingdone` is sent for faces that failed too: one that cannot load does not hold the report.
    const sync = () => setLoaded(fonts.status !== 'loading');
    fonts.addEventListener('loading', sync);
    fonts.addEventListener('loadingdone', sync);
    sync();

    return () => {
      fonts.removeEventListener('loading', sync);
      fonts.removeEventListener('loadingdone', sync);
    };
  }, []);

  return loaded;
}

function fontSet(): FontFaceSet | undefined {
  return typeof document === 'undefined' ? undefined : document.fonts;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error) return error;
  return 'report could not be rendered';
}
