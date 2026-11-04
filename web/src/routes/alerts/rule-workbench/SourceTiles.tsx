import { HeartPulse, MonitorSmartphone } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';

import { type RuleSignal, SIGNAL_ICON } from './presets';
import type { SourceKind, TypedSource } from './typedSource';

const SIGNALS: RuleSignal[] = ['metrics', 'logs', 'traces'];

function Tile({
  active,
  onClick,
  icon,
  label,
  source,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  /** Which product's figure the tile chooses, for the tiles that are not a query's signal. */
  source?: SourceKind;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      {...(source ? { 'data-alert-source': source } : {})}
      className={cn(
        'flex min-h-14 items-center gap-3 bg-bg-2 px-3 text-left transition-colors',
        'focus-visible:bg-indigo-dim focus-visible:text-indigo',
        active ? 'bg-indigo-dim text-indigo-soft' : 'text-tx-2 hover:bg-bg-3 hover:text-tx-0',
      )}
    >
      {icon}
      <span className="font-sans text-sm font-strong">{label}</span>
    </button>
  );
}

/**
 * What a rule watches: the result of a query on one of the three signals, or a figure of APM or
 * of RUM. Exactly one of them is lit.
 */
export function SourceTiles({
  signal,
  source,
  onSignal,
  onSource,
  typed = true,
}: {
  signal: RuleSignal;
  source: TypedSource | null;
  onSignal: (signal: RuleSignal) => void;
  onSource: (kind: SourceKind) => void;
  /** Offer APM and RUM figures; an outlier rule compares groups of a query and offers only signals. */
  typed?: boolean;
}) {
  const { t } = useTranslation('alerts');

  return (
    <div>
      <div className={uiLabelClass}>{t('workbench.fields.signal')}</div>
      <div className="mt-2 grid grid-cols-1 gap-2 sm:grid-cols-5">
        {SIGNALS.map((item) => (
          <Tile
            key={item}
            active={signal === item && !source}
            onClick={() => onSignal(item)}
            icon={SIGNAL_ICON[item]}
            label={t(`workbench.signals.${item}`)}
          />
        ))}
        {typed && (
          <>
            <Tile
              active={source?.kind === 'apm'}
              onClick={() => onSource('apm')}
              icon={<HeartPulse className="h-4 w-4" />}
              label={t('workbench.signals.apm')}
              source="apm"
            />
            <Tile
              active={source?.kind === 'rum'}
              onClick={() => onSource('rum')}
              icon={<MonitorSmartphone className="h-4 w-4" />}
              label={t('workbench.signals.rum')}
              source="rum"
            />
          </>
        )}
      </div>
    </div>
  );
}
