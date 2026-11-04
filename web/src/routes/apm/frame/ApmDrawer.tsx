import { X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { ResizeHandle } from '@/shell/ResizeHandle';
import { Button } from '@/shell/ui/button';
import { Sheet, SheetClose, SheetContent, SheetTitle } from '@/shell/ui/sheet';
import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

import { ApmDrawerCards } from './Page';

/** The narrowest an APM drawer is drawn, and its share of the window until the reader sets one. */
const MIN_WIDTH = 640;
const FALLBACK_SHARE = 0.8;
/** The room the list keeps beside the drawer at the least. */
const LIST_ROOM = 48;

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
 * An endpoint or an error group opened from a list without leaving it: a
 * drawer most of the window wide whose left edge the reader drags, the same
 * as the trace drawer. One width is kept for every APM drawer.
 */
export function ApmDrawer({
  open,
  onClose,
  title,
  subtitle,
  trail,
  actions,
  testId,
  children,
}: {
  open: boolean;
  onClose: () => void;
  title: React.ReactNode;
  /** The drawers open under this one, each a way back to it (`DrawerTrail`). */
  trail?: React.ReactNode;
  subtitle?: React.ReactNode;
  actions?: React.ReactNode;
  testId: string;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('apm');
  const kept = useUiPreference('apm.layout', 'view', parseLayout);
  const [live, setLive] = React.useState<number | null>(null);
  const windowWidth = useWindowWidth();
  const fallback = Math.round(windowWidth * FALLBACK_SHARE);
  const maxWidth = Math.max(MIN_WIDTH, windowWidth - LIST_ROOM);
  const width = Math.min(maxWidth, live ?? kept.value?.drawer ?? fallback);

  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent
        side="right"
        hideClose
        overlayClassName="top-topbar bg-overlay-soft"
        aria-describedby={undefined}
        data-apm-drawer={testId}
        className={cn('bottom-0 top-topbar flex h-auto max-w-none flex-col gap-0 border-0 bg-[var(--functional-surface)] p-0 shadow-drawer sm:max-w-none')}
        style={{ width }}
      >
        <div className="absolute inset-y-0 left-0 z-30 -translate-x-1/2">
          <ResizeHandle
            label={t('drawer.resize')}
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
        <header className="flex shrink-0 items-start gap-3 border-b border-bd-0 px-4 py-[10px]">
          <div className="min-w-0 flex-1">
            {trail && <div className="mb-1 empty:hidden">{trail}</div>}
            <SheetTitle className="type-section-title truncate font-sans font-display text-tx-0">{title}</SheetTitle>
            {subtitle && <div className="mt-0.5 truncate font-sans text-xs text-tx-2">{subtitle}</div>}
          </div>
          <div className="flex shrink-0 items-center gap-1">
            {actions}
            <SheetClose asChild>
              <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('drawer.close')}>
                <X className="h-4 w-4" />
              </Button>
            </SheetClose>
          </div>
        </header>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
          <ApmDrawerCards>{children}</ApmDrawerCards>
        </div>
      </SheetContent>
    </Sheet>
  );
}
