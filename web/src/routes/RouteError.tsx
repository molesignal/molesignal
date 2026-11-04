import { Home, RotateCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { isRouteErrorResponse, Link, useLocation, useRouteError } from 'react-router-dom';

import { writeClipboardText } from '@/lib/clipboard';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { SurfaceStatePage } from '@/shell/SurfaceWorkbench';

/** A page's code from an older build, gone from the server since it was loaded. */
const STALE_BUILD = /dynamically imported module|importing a module script failed/i;

function errorText(error: unknown): { message: string; stack: string | undefined } {
  if (isRouteErrorResponse(error)) {
    return { message: `${error.status} ${error.statusText}`, stack: undefined };
  }
  if (error instanceof Error) return { message: error.message, stack: error.stack };
  return { message: String(error), stack: undefined };
}

/**
 * What a route shows when it throws while drawing. Under the shell the sidebar
 * and the top bar stay, so the way to any other page is still there; only what
 * sits above the shell takes the whole window. React Router clears it on the
 * next navigation.
 */
export function RouteError({ fullPage = false }: { fullPage?: boolean }) {
  const { t } = useTranslation('shell');
  const location = useLocation();
  const failure = useRouteError();
  const [copied, setCopied] = React.useState(false);

  const { message, stack } = errorText(failure);
  const staleBuild = STALE_BUILD.test(message);

  const copyReport = async () => {
    const report = [
      `${location.pathname}${location.search}`,
      new Date().toISOString(),
      navigator.userAgent,
      message,
      stack,
    ]
      .filter(Boolean)
      .join('\n');

    try {
      await writeClipboardText(report);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // The clipboard is blocked: the message stays on screen.
    }
  };

  const state = (
    <ProductState
      variant="error"
      data-testid="route-error"
      title={t(staleBuild ? 'route_error.stale_title' : 'route_error.title')}
      description={
        <>
          {t(staleBuild ? 'route_error.stale_description' : 'route_error.description')}
          {!staleBuild && (
            <span className="mt-2 block break-words font-mono text-xs text-tx-3">{message}</span>
          )}
        </>
      }
      action={
        <>
          <ChromeButton variant="primary" onClick={() => window.location.reload()}>
            <RotateCw className="h-3.5 w-3.5" />
            {t('route_error.reload')}
          </ChromeButton>
          <Link
            to="/home"
            className="ms-press inline-flex h-9 items-center gap-2 rounded-md bg-bg-2 px-4 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none"
          >
            <Home className="h-3.5 w-3.5" aria-hidden />
            {t('route_error.home')}
          </Link>
          {!staleBuild && (
            <CopyIconButton
              onClick={() => void copyReport()}
              label={t('route_error.copy_report')}
              copied={copied}
              copiedLabel={t('route_error.copied')}
            />
          )}
        </>
      }
    />
  );

  if (fullPage) {
    return <div className="grid min-h-screen place-items-center bg-[var(--page-canvas)] p-6">{state}</div>;
  }

  return <SurfaceStatePage>{state}</SurfaceStatePage>;
}
