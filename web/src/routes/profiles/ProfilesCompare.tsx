import { useQuery } from '@tanstack/react-query';
import { Play } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as profilesApi from '@/api/profiles';
import { toApiError } from '@/lib/http';
import { type ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { Button } from '@/shell/ui/button';
import { resolveExpr } from '@/stores/useTimeStore';
import { Flamegraph } from '@/viz/profiles/Flamegraph';

import { DurationSelect, ServiceSelect, TruncatedNotice, TypeSelect } from './shared';

function durationMicros(expr: string): number {
  const now = Date.now();
  const start = resolveExpr(expr, new Date()).getTime();
  return Math.max(1, (now - start) * 1000);
}

export function ProfilesCompare() {
  const { t } = useTranslation('profiles');
  const { t: tEdition } = useTranslation('edition');
  const [service, setService] = React.useState('');
  const [type, setType] = React.useState('cpu');
  const [duration, setDuration] = React.useState('now-1h');
  const [submitted, setSubmitted] = React.useState(0);

  // Populate the service dropdown from recent profiles (last 24h).
  const servicesQuery = useQuery({
    queryKey: ['profiles-compare-services'],
    queryFn: async () => {
      const now = Date.now() * 1000;
      const rows = await profilesApi.list({ from: now - 24 * 3_600 * 1_000_000, to: now, limit: 500 });
      return Array.from(new Set(rows.map((r) => r.service).filter(Boolean))).sort();
    },
  });

  const diffQuery = useQuery({
    queryKey: ['profiles-diff', service, type, duration, submitted],
    enabled: submitted > 0,
    queryFn: () => {
      const dur = durationMicros(duration);
      const nowMicros = Date.now() * 1000;
      return profilesApi.diff({
        service: service || undefined,
        type: type || undefined,
        from: nowMicros - dur,
        to: nowMicros,
        baseline_from: nowMicros - 2 * dur,
        baseline_to: nowMicros - dur,
      });
    },
  });

  // Diff is a license-gated enhancement; the backend answers 402/403 when the
  // edition lacks it. Surface that as an edition gate, never a bare error.
  let errorState: ProductStateProps | null = null;
  if (diffQuery.isError) {
    const status = toApiError(diffQuery.error).status;
    if (status === 402 || status === 403) {
      const feature = tEdition('features.profiling_enhanced');
      errorState = {
        variant: 'pro-required',
        title: tEdition('gates.pro-required.title', { feature }),
        description: tEdition('gates.pro-required.description', { feature }),
      };
    } else {
      errorState = { variant: 'error', error: diffQuery.error };
    }
  }

  const filters = (
    <div className="flex flex-wrap items-center gap-2">
      <ServiceSelect value={service} services={servicesQuery.data ?? []} onChange={setService} />
      <TypeSelect value={type} onChange={setType} />
      <DurationSelect value={duration} onChange={setDuration} ariaLabel={t('filters.range')} />
      <Button size="sm" onClick={() => setSubmitted((n) => n + 1)} disabled={diffQuery.isFetching}>
        <Play className="h-3.5 w-3.5" /> {t('compare.run')}
      </Button>
    </div>
  );

  return (
    <ListPage
      title={t('compare.title')}
      subtitle={t('compare.subtitle') as string}
      filters={filters}
      breadcrumbs={[{ labelKey: 'profiles', to: '/profiles' }, { labelKey: 'breadcrumbs.profiles_compare' }]}
      backTo="/profiles"
      state={
        diffQuery.isFetching
          ? { variant: 'loading' }
          : errorState ?? (submitted === 0
              ? {
                  variant: 'empty',
                  title: t('compare.empty_title'),
                  description: t('compare.empty_description'),
                }
              : null)
      }
    >
      {diffQuery.data && (
        <div className="space-y-4">
          {diffQuery.data.truncated && <TruncatedNotice />}
          <div className="flex flex-wrap gap-4 font-sans text-xs text-tx-2">
            <span>{t('compare.baseline_count', { count: diffQuery.data.baseline_count })}</span>
            <span>{t('compare.comparison_count', { count: diffQuery.data.comparison_count })}</span>
          </div>
          <Flamegraph flamebearer={diffQuery.data.flamebearer} diff />
        </div>
      )}
    </ListPage>
  );
}
