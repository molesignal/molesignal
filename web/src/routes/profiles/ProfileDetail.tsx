import { useQuery } from '@tanstack/react-query';
import { Download, ExternalLink } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams } from 'react-router-dom';

import * as profilesApi from '@/api/profiles';
import type { ProfileEntry } from '@/api/profiles';
import { formatMicrosActive } from '@/lib/time';
import { type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { Button } from '@/shell/ui/button';
import { formatCount, formatDuration } from '@/viz/profiles/flamebearer';
import { Flamegraph } from '@/viz/profiles/Flamegraph';

import { useProfileTypeLabel } from './shared';

const WIDE_LOOKUP_MICROS = 7 * 24 * 3_600 * 1_000_000;
const FLAME_PAD_MICROS = 5 * 60 * 1_000_000;

export function ProfileDetail() {
  const { t } = useTranslation('profiles');
  const { id = '' } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const typeLabel = useProfileTypeLabel();
  const stateProfile = (location.state as { profile?: ProfileEntry } | null)?.profile;
  const [downloading, setDownloading] = React.useState(false);

  // Deep-link fallback: the backend has no by-id metadata endpoint, so scan a
  // wide window when we didn't arrive with the row in router state.
  const lookupQuery = useQuery({
    queryKey: ['profile-detail', id],
    enabled: !stateProfile && id.length > 0,
    queryFn: async () => {
      const now = Date.now() * 1000;
      const rows = await profilesApi.list({ from: now - WIDE_LOOKUP_MICROS, to: now, limit: 1000 });
      return rows.find((r) => r.id === id) ?? null;
    },
  });
  const profile = stateProfile ?? lookupQuery.data ?? null;

  const flameQuery = useQuery({
    queryKey: ['profile-detail-flame', id, profile?.trace_id, profile?.span_id],
    enabled: profile !== null,
    queryFn: () => {
      if (!profile) throw new Error('no profile');
      if (profile.trace_id) {
        return profilesApi.flamegraph({
          trace_id: profile.trace_id,
          ...(profile.span_id ? { span_id: profile.span_id } : {}),
        });
      }
      return profilesApi.flamegraph({
        service: profile.service,
        type: profile.profile_type,
        from: profile.timestamp - FLAME_PAD_MICROS,
        to: profile.timestamp + FLAME_PAD_MICROS,
      });
    },
  });

  const handleDownload = async () => {
    if (!id) return;
    setDownloading(true);
    try {
      const name = profile ? `${profile.service}-${profile.profile_type}-${id}.pprof.gz` : undefined;
      await profilesApi.download(id, name);
    } finally {
      setDownloading(false);
    }
  };

  const openTrace = () => {
    if (!profile?.trace_id) return;
    const clause = `trace_id = '${profile.trace_id.replace(/'/g, "\\'")}'`;
    navigate(`/traces?q=${encodeURIComponent(clause)}`);
  };

  const loading = !profile && lookupQuery.isLoading;
  const notFound = !profile && !lookupQuery.isLoading;
  const detailState: ProductStateProps | null = loading
    ? { variant: 'loading' }
    : notFound
      ? { variant: 'empty', title: t('detail.title'), description: t('list.empty_title') }
      : null;

  const toolbar = profile ? (
    <div className="flex items-center gap-2">
      {profile.trace_id && (
        <Button variant="outline" size="sm" onClick={openTrace}>
          <ExternalLink className="h-3.5 w-3.5" /> {t('detail.open_trace')}
        </Button>
      )}
      <Button size="sm" onClick={() => void handleDownload()} disabled={downloading}>
        <Download className="h-3.5 w-3.5" /> {downloading ? t('detail.downloading') : t('detail.download')}
      </Button>
    </div>
  ) : undefined;

  return (
    <DetailPage
      title={profile ? `${profile.service} · ${typeLabel(profile.profile_type)}` : t('detail.title')}
      toolbar={toolbar}
      breadcrumbs={[{ labelKey: 'profiles', to: '/profiles' }, { labelKey: 'breadcrumbs.profile_detail' }]}
      backTo="/profiles"
      metadata={
        profile
          ? [
              { label: t('detail.metadata.service'), value: profile.service },
              { label: t('detail.metadata.type'), value: typeLabel(profile.profile_type) },
              { label: t('detail.metadata.captured'), value: formatMicrosActive(profile.timestamp) },
              { label: t('detail.metadata.duration'), value: formatDuration(profile.duration_nanos) },
              { label: t('detail.metadata.samples'), value: formatCount(profile.sample_count) },
              ...(profile.trace_id ? [{ label: t('detail.metadata.trace'), value: profile.trace_id.slice(0, 16) }] : []),
            ]
          : undefined
      }
      state={detailState}
    >
      {profile && (
        <div className="space-y-3">
          {profile.unsymbolized && (
            <div className="rounded-md border border-yellow/30 bg-yellow-dim px-3 py-2 font-sans text-xs text-yellow-soft">
              {t('detail.unsymbolized')}
            </div>
          )}
          {flameQuery.isError ? (
            <div className="rounded-md border border-bd-0 bg-bg-1 p-6 text-center font-sans text-xs text-tx-2">
              {t('errors.flamegraph_failed')}
            </div>
          ) : flameQuery.data ? (
            <Flamegraph flamebearer={flameQuery.data.flamebearer} />
          ) : (
            <div className="rounded-md border border-bd-0 bg-bg-1 p-6 text-center font-sans text-xs text-tx-2">
              {t('flamegraph.no_data_title')}
            </div>
          )}
        </div>
      )}
    </DetailPage>
  );
}
