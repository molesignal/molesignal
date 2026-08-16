import {
  ArrowLeft,
  ArrowRight,
  Eye,
  EyeOff,
  Maximize2,
  Minimize2,
  RotateCcw,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { useNocLayoutStore } from '@/stores/useNocLayoutStore';

export function NocEditBar({ onDone }: { onDone: () => void }) {
  const { t } = useTranslation('shell');
  const panels = useNocLayoutStore((state) => state.panels);
  const move = useNocLayoutStore((state) => state.move);
  const toggleVisible = useNocLayoutStore((state) => state.toggleVisible);
  const cycleSpan = useNocLayoutStore((state) => state.cycleSpan);
  const applyPreset = useNocLayoutStore((state) => state.applyPreset);
  const reset = useNocLayoutStore((state) => state.reset);

  return (
    <div className="mb-1 flex flex-wrap items-center gap-2 rounded-lg border border-bd-1 bg-bg-1 px-3 py-2 font-sans text-xs">
      <span className="font-strong uppercase tracking-normal text-tx-2">
        {t('pages.noc.edit.presets')}
      </span>
      {(['platform', 'sre', 'executive'] as const).map((preset) => (
        <button
          key={preset}
          type="button"
          onClick={() => applyPreset(preset)}
          className="rounded border border-bd-1 bg-bg-2 px-2 py-1 font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none"
        >
          {t(`pages.noc.edit.preset_${preset}`)}
        </button>
      ))}
      <span className="mx-1 h-4 w-px bg-bd-1" />
      {panels.map((panel, index) => (
        <div
          key={panel.id}
          className="flex items-center gap-1 rounded border border-bd-0 bg-bg-2 px-1.5 py-1"
        >
          <span
            className={cn(
              'mr-1 font-strong',
              panel.visible ? 'text-tx-1' : 'text-tx-2 line-through',
            )}
          >
            {t(`pages.noc.panel_names.${panel.id}`)}
          </span>
          <NocEditButton
            onClick={() => move(panel.id, -1)}
            disabled={index === 0}
            label={t('pages.noc.edit.move_left')}
          >
            <ArrowLeft className="h-3 w-3" />
          </NocEditButton>
          <NocEditButton
            onClick={() => move(panel.id, 1)}
            disabled={index === panels.length - 1}
            label={t('pages.noc.edit.move_right')}
          >
            <ArrowRight className="h-3 w-3" />
          </NocEditButton>
          <NocEditButton
            onClick={() => cycleSpan(panel.id)}
            label={t('pages.noc.edit.wide')}
          >
            {panel.span === 4 ? (
              <Minimize2 className="h-3 w-3" />
            ) : (
              <Maximize2 className="h-3 w-3" />
            )}
          </NocEditButton>
          <NocEditButton
            onClick={() => toggleVisible(panel.id)}
            label={t('pages.noc.edit.show_hide')}
          >
            {panel.visible ? (
              <Eye className="h-3 w-3" />
            ) : (
              <EyeOff className="h-3 w-3" />
            )}
          </NocEditButton>
        </div>
      ))}
      <div className="ml-auto flex items-center gap-2">
        <button
          type="button"
          onClick={reset}
          className="inline-flex items-center gap-1 rounded px-2 py-1 font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none"
        >
          <RotateCcw className="h-3 w-3" />
          {t('pages.noc.edit.reset')}
        </button>
        <button
          type="button"
          onClick={onDone}
          className="rounded bg-indigo px-2.5 py-1 font-bold text-white hover:brightness-90 focus-visible:brightness-90 focus-visible:text-white focus-visible:outline-none"
        >
          {t('pages.noc.edit.done')}
        </button>
      </div>
    </div>
  );
}

function NocEditButton({
  children,
  onClick,
  disabled,
  label,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      title={label}
      className="grid h-5 w-5 place-items-center rounded text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-indigo-dim focus-visible:text-indigo focus-visible:outline-none disabled:opacity-30 disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}
