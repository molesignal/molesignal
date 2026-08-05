import { MonitorX } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { LogoMark } from '@/shell/LogoMark';

/**
 * Full-screen interstitial shown when the viewport is narrower than the
 * 1024px desktop minimum. Molesignal's dense SRE surfaces — multi-pane
 * investigation, the NOC wallboard, the dashboard editor — have no mobile
 * fallback by design; we ask the operator to widen rather than silently
 * degrade into a broken layout.
 */
export function UnsupportedScreen({ width }: { width: number }) {
  const { t } = useTranslation('shell');
  return (
    <div className="grid min-h-screen place-items-center bg-bg-0 px-6 text-tx-0">
      <div className="flex max-w-sm flex-col items-center gap-4 text-center">
        <div className="flex items-center gap-2">
          <LogoMark size={26} />
          <span className="font-sans text-base font-bold tracking-tight text-tx-0">MoleSignal</span>
        </div>
        <div className="grid h-12 w-12 place-items-center rounded-full bg-indigo-dim">
          <MonitorX className="h-6 w-6 text-indigo-soft" />
        </div>
        <h1 className="font-sans text-lg font-display-strong text-tx-0">{t('unsupported.title')}</h1>
        <p className="font-sans text-sm leading-relaxed text-tx-2">{t('unsupported.body')}</p>
        <p className="font-mono text-xs text-tx-3">{t('unsupported.width', { width })}</p>
      </div>
    </div>
  );
}
