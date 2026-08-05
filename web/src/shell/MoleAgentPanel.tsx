import { Bot, ExternalLink, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';

// Lazy so the Mole Intelligence chat module stays out of the shell bundle until
// the operator first opens Mole Agent.
const LazyAgentChat = React.lazy(() =>
  import('@/routes/intelligence').then((module) => ({ default: module.IntelligenceChat })),
);

/**
 * Shell-level Mole Agent — a right-side slide-out that hosts the full chat
 * experience (reused from `/intelligence/chat` in embedded single-column mode) so
 * an operator can ask a question without navigating away from the alert /
 * trace / dashboard they're looking at. Toggled by the Topbar ✨ button or ⌘J;
 * ESC closes. It overlays the content (no backdrop) so the page underneath
 * stays interactive.
 */
export function MoleAgentPanel() {
  const { t } = useTranslation('shell');
  const nav = useNavigate();
  const location = useLocation();
  const isOpen = useMoleAgentStore((s) => s.isOpen);
  const close = useMoleAgentStore((s) => s.close);
  const timeWindow = useTimeStore((s) => s.window);

  React.useEffect(() => {
    if (!isOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isOpen, close]);

  const openFull = () => {
    close();
    nav('/intelligence/chat');
  };

  return (
    <aside
      aria-hidden={!isOpen}
      aria-label={t('agent_panel.title')}
      className={cn(
        'fixed bottom-0 right-0 top-topbar z-40 flex w-[640px] max-w-[92vw] flex-col border-l border-bd-0 bg-bg-0 shadow-drawer',
        'transition-transform duration-normal ease-out-default',
        isOpen ? 'translate-x-0' : 'pointer-events-none translate-x-full',
      )}
    >
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-bd-0 bg-bg-1 px-3">
        <Bot className="h-3.5 w-3.5 shrink-0 text-indigo-soft" />
        <span className="font-sans text-xs font-strong text-tx-1">{t('agent_panel.title')}</span>
        <button
          type="button"
          onClick={openFull}
          className="ml-auto inline-flex items-center gap-1 rounded px-1.5 py-0.5 font-sans text-xs text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
          title={t('agent_panel.open_full')}
        >
          <ExternalLink className="h-3 w-3" />
          <span className="hidden sm:inline">{t('agent_panel.open_full')}</span>
        </button>
        <button
          type="button"
          onClick={close}
          aria-label={t('agent_panel.close')}
          title={t('agent_panel.close')}
          className="grid h-8 w-8 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>

      <div className="type-micro flex h-6 shrink-0 items-center gap-1.5 overflow-hidden border-b border-bd-0 bg-bg-2 px-3 font-mono text-tx-3">
        <span className="shrink-0 uppercase tracking-normal text-tx-2">{t('agent_panel.context_label')}</span>
        <span className="truncate">{location.pathname}</span>
        <span className="text-tx-3">·</span>
        <span className="shrink-0">{formatWindowSummary(timeWindow)}</span>
      </div>

      <div className="min-h-0 flex-1 overflow-hidden">
        {isOpen && (
          <React.Suspense
            fallback={
              <div className="flex h-full items-center justify-center font-sans text-xs text-tx-3">
                {t('agent_panel.loading')}
              </div>
            }
          >
            <LazyAgentChat embedded />
          </React.Suspense>
        )}
      </div>
    </aside>
  );
}
