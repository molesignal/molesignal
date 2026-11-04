import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ResizeHandle } from '@/shell/ResizeHandle';
import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

/** The narrowest a log is read at, and its share of the window until the reader sets one. */
const MIN_WIDTH = 440;
const FALLBACK_SHARE = 0.45;
/** The room the list keeps beside the drawer at the least. */
const LIST_ROOM = 320;

function parseLayout(raw: unknown): { drawer?: number } | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const drawer = (raw as Record<string, unknown>).drawer;
  return typeof drawer === 'number' && Number.isFinite(drawer) && drawer >= 160 && drawer <= 4_000 ? { drawer: Math.round(drawer) } : {};
}

function useWindowWidth(): number {
  const [width, setWidth] = React.useState(() => (typeof window === 'undefined' ? 1440 : window.innerWidth));

  React.useEffect(() => {
    const measure = () => setWidth(window.innerWidth);
    window.addEventListener('resize', measure);
    return () => window.removeEventListener('resize', measure);
  }, []);

  return width;
}

/**
 * The drawer a log is read in, over the right of the results as Datadog's
 * log panel is: wide enough for a log and its trace, the list still in sight
 * beside it. Its left edge is dragged; the width is kept for the reader.
 */
export function LogDetailFrame({ label, children }: { label: string; children: React.ReactNode }) {
  const { t } = useTranslation('logs');
  const kept = useUiPreference('logs.layout', 'view', parseLayout);
  const [live, setLive] = React.useState<number | null>(null);
  const windowWidth = useWindowWidth();
  const maxWidth = Math.max(MIN_WIDTH, windowWidth - LIST_ROOM);
  const fallback = Math.min(maxWidth, Math.max(MIN_WIDTH, Math.round(windowWidth * FALLBACK_SHARE)));
  const width = Math.min(maxWidth, Math.max(MIN_WIDTH, live ?? kept.value?.drawer ?? fallback));

  return (
    <aside
      aria-label={label}
      data-state="open"
      data-log-detail-frame=""
      style={{ width }}
      className="fixed bottom-0 right-0 top-topbar z-[60] min-h-0 max-w-full bg-[var(--functional-surface)] shadow-drawer data-[state=open]:animate-slide-in-right"
    >
      <div className="absolute inset-y-0 left-0 z-30 -translate-x-1/2">
        <ResizeHandle
          label={t('explore.detail.resize')}
          width={width}
          min={MIN_WIDTH}
          max={maxWidth}
          grows="left"
          defaultWidth={fallback}
          onWidthChange={setLive}
          onWidthCommit={(drawer) => {
            setLive(drawer);
            kept.save({ drawer });
          }}
          quiet
          className="h-full"
        />
      </div>
      {children}
    </aside>
  );
}
