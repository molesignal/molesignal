import { useQuery } from '@tanstack/react-query';
import { ExternalLink, Flame } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as profilesApi from '@/api/profiles';
import type { ProfileEntry } from '@/api/profiles';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Button } from '@/shell/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/shell/ui/dialog';
import { formatCount, formatValue, topFunctions } from '@/viz/profiles/flamebearer';
import { Flamegraph } from '@/viz/profiles/Flamegraph';

import { profilesHref, profileWindow, type TraceProfiles } from './profileLinks';
import type { TraceNode } from '../model/tree';

/** The types of profile in the order they are offered, as the Profiles page offers them. */
const TYPE_ORDER = ['cpu', 'wall', 'alloc_space', 'alloc_objects', 'inuse_space', 'inuse_objects', 'goroutines', 'lock'];
/** As many functions as the tab lists. */
const TOP = 5;

const LINK = 'inline-flex items-center gap-1 rounded font-sans text-xs text-[var(--blue-ink)] hover:underline focus-visible:underline focus-visible:outline-none';

function typesOf(profiles: readonly ProfileEntry[]): string[] {
  const present = new Set(profiles.map((profile) => profile.profileType));
  return [...TYPE_ORDER.filter((type) => present.has(type)), ...[...present].filter((type) => !TYPE_ORDER.includes(type)).sort()];
}

/**
 * What the profiler saw while the span ran. With samples that kept the span's
 * id, the functions that took most of them, and their flame graph, one type
 * of profile at a time: seconds of CPU and bytes of memory do not add up.
 * Without, the profiles of the span's time, on the Profiles page.
 */
export function SpanProfile({
  traceId,
  node,
  profiles,
  loading,
}: {
  traceId: string;
  node: TraceNode;
  /** The trace's profiles, once they are read. */
  profiles: TraceProfiles | undefined;
  loading: boolean;
}) {
  const { t } = useTranslation(['traces', 'profiles']);
  const linked = React.useMemo(() => profiles?.bySpan.get(node.id) ?? [], [node.id, profiles]);
  const types = React.useMemo(() => typesOf(linked), [linked]);
  const [chosen, setChosen] = React.useState<string | null>(null);
  const type = chosen && types.includes(chosen) ? chosen : types[0] ?? '';
  const [open, setOpen] = React.useState(false);
  const window = profiles?.window ?? profileWindow(node.startNs, node.endNs);
  const typeLabel = (key: string) => (TYPE_ORDER.includes(key) ? t(`profiles:profile_types.${key}`) : key);

  const flame = useQuery({
    queryKey: ['trace', 'span-profile', traceId, node.id, type, window.from, window.to],
    queryFn: () => profilesApi.flamegraph({ traceId: traceId, spanId: node.id, type, from: window.from, to: window.to }),
    enabled: linked.length > 0 && type !== '',
    staleTime: 60_000,
  });

  if (loading) return <p className="font-sans text-xs text-tx-3">{t('view.profile.loading')}</p>;

  if (linked.length === 0) {
    return (
      <div data-testid="trace-span-profile" data-profile="none" className="space-y-2 font-sans text-xs">
        <p className="text-tx-1">{t('view.profile.none')}</p>
        <p className="text-tx-3">{t('view.profile.none_hint')}</p>
        <Link to={profilesHref(profileWindow(node.startNs, node.endNs))} className={LINK} data-testid="trace-span-profile-window">
          {t('view.profile.around')}
          <ExternalLink aria-hidden="true" className="h-3 w-3" />
        </Link>
      </div>
    );
  }

  const ofType = linked.filter((profile) => profile.profileType === type);
  const samples = ofType.reduce((total, profile) => total + (profile.sampleCount || 0), 0);
  const fb = flame.data?.flamebearer;
  const top = fb ? topFunctions(fb).slice(0, TOP) : [];

  return (
    <div data-testid="trace-span-profile" data-profile="linked" className="space-y-2 font-sans text-xs">
      <p className="text-tx-1">
        {t('view.profile.linked', { count: linked.length })}
        <span className="ml-1 text-tx-3">{t('view.profile.samples', { count: samples, samples: formatCount(samples) })}</span>
      </p>
      {types.length > 1 && (
        <ViewTabs label={t('view.profile.types')} value={type} onValueChange={setChosen} items={types.map((key) => ({ key, label: typeLabel(key) }))} />
      )}
      <section aria-label={t('view.profile.top')}>
        <h3 className="mb-1 font-strong text-tx-2">{t('view.profile.top_of', { type: typeLabel(type) })}</h3>
        {flame.isLoading ? (
          <p className="text-tx-3">{t('view.profile.loading')}</p>
        ) : flame.isError ? (
          <p className="text-[var(--red-ink)]">{t('view.profile.failed')}</p>
        ) : top.length === 0 ? (
          <p className="text-tx-3">{t('view.profile.empty')}</p>
        ) : (
          <ol className="flex flex-col" data-testid="trace-span-profile-top">
            {top.map((row) => (
              <li key={row.name} className="flex h-7 min-w-0 items-center gap-2 border-b border-bd-0">
                <span className="min-w-0 flex-1 truncate text-tx-0" title={row.name}>{row.name}</span>
                <span className="shrink-0 tabular-nums text-tx-2">{formatValue(row.self, fb!.units)}</span>
                <span className="w-12 shrink-0 text-right font-strong tabular-nums text-tx-1">{Math.round(row.selfPct)}%</span>
              </li>
            ))}
          </ol>
        )}
      </section>
      <div className="flex flex-wrap items-center gap-3 pt-1">
        <Button size="sm" variant="outline" className="h-7" disabled={!fb || top.length === 0} onClick={() => setOpen(true)} data-testid="trace-span-profile-open">
          <Flame aria-hidden="true" className="h-3.5 w-3.5" />
          {t('view.profile.open_flame')}
        </Button>
        <Link to={profilesHref(window, { traceId, spanId: node.id })} className={LINK} data-testid="trace-span-profile-page">
          {t('view.profile.open_page')}
          <ExternalLink aria-hidden="true" className="h-3 w-3" />
        </Link>
      </div>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="max-h-[calc(100vh-48px)] w-[min(1200px,calc(100vw-48px))] max-w-none grid-rows-[auto_minmax(0,1fr)] overflow-hidden">
          <div className="min-w-0 pr-6">
            <DialogTitle className="font-sans text-sm font-strong">{t('view.profile.dialog_title', { operation: node.span.operation })}</DialogTitle>
            <DialogDescription className="mt-1 font-sans text-xs text-tx-2">
              {t('view.profile.dialog_description', { service: node.service, type: typeLabel(type), count: ofType.length })}
            </DialogDescription>
          </div>
          <div className="min-h-0 overflow-auto">
            {fb && <Flamegraph flamebearer={fb} service={node.service} className="border-0" />}
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
