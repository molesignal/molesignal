import { Radio, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Input } from '@/shell/ui/input';

import { ServiceSelect, TypeSelect } from '../shared';

/**
 * What the analysis is narrowed to (service, profile type, a label) and how it
 * is kept fresh. One compact row: every control names itself, so none needs a
 * caption over it.
 */
export function ProfileFilterBar({
  service,
  services,
  type,
  presentTypes,
  label,
  live,
  refetching,
  onServiceChange,
  onTypeChange,
  onLabelChange,
  onRefresh,
  onLiveChange,
}: {
  service: string;
  services: string[];
  type: string;
  /** The types the window has profiles of, where it has been listed whole. */
  presentTypes?: readonly string[] | undefined;
  label: string;
  live: boolean;
  refetching: boolean;
  onServiceChange: (value: string) => void;
  onTypeChange: (value: string) => void;
  onLabelChange: (value: string) => void;
  onRefresh: () => void;
  onLiveChange: (live: boolean) => void;
}) {
  const { t } = useTranslation('profiles');

  return (
    <section
      aria-label={t('analysis.filter_bar_aria')}
      className="flex min-w-0 flex-wrap items-center gap-2 rounded-md bg-[var(--functional-surface)] px-[12px] py-[8px] [box-shadow:var(--shadow-functional-surface)]"
    >
      <ServiceSelect value={service} services={services} onChange={onServiceChange} />
      <TypeSelect value={type} onChange={onTypeChange} present={presentTypes} />
      <Input
        value={label}
        onChange={(event) => onLabelChange(event.target.value)}
        placeholder={`${t('filters.label')}: ${t('filters.label_placeholder')}`}
        aria-label={t('filters.label')}
        className="h-8 min-w-[180px] flex-1 font-mono text-xs lg:max-w-[280px]"
      />
      <div className="ml-auto flex items-center gap-2">
        <ChromeButton size="sm" onClick={onRefresh}>
          <RefreshCw className={cn('h-3.5 w-3.5', refetching && 'animate-spin')} />
          {t('refresh')}
        </ChromeButton>
        <ChromeButton
          size="sm"
          variant={live ? 'primary' : 'default'}
          onClick={() => onLiveChange(!live)}
          title={t('live.hint')}
          aria-pressed={live}
        >
          <Radio className={cn('h-3.5 w-3.5', live && 'animate-pulse')} />
          {t('live.label')}
        </ChromeButton>
      </div>
    </section>
  );
}
