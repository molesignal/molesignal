import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as versionApi from '@/api/version';
import { ChromeButton } from '@/shell/chrome';

import { RELEASE_UPDATES_KEY, useReleaseUpdates } from './useReleaseUpdates';

export function ReleaseUpdatePanel({ open }: { open: boolean }) {
  const { t } = useTranslation('shell');
  const query = useReleaseUpdates(open);
  const client = useQueryClient();
  const refresh = useMutation({
    mutationFn: () => versionApi.updates(true),
    onSuccess: (data) => client.setQueryData(RELEASE_UPDATES_KEY, data),
  });
  const data = query.data;
  const available = !!data?.updateAvailable;
  const unavailable = query.isError || refresh.isError || data?.status === 'unavailable';
  const checking = query.isFetching || refresh.isPending;

  return (
    <div className="mt-4 rounded-lg bg-bg-2 px-3 py-3" data-testid="release-update-panel">
      <p className="text-sm text-tx-1" role="status">
        {available
          ? t('about.updates.available', { version: data.latestVersion })
          : checking
            ? t('about.updates.checking')
            : unavailable || !data
              ? t('about.updates.unavailable')
              : t('about.updates.current')}
      </p>
      {available && unavailable && (
        <p className="mt-1 text-xs text-tx-3">{t('about.updates.stale')}</p>
      )}
      <div className="mt-2 flex items-center gap-3">
        {available && data?.releaseUrl && (
          <a className="text-sm text-indigo hover:underline focus-visible:bg-indigo-dim"
            href={data.releaseUrl} target="_blank" rel="noopener noreferrer">
            {t('about.updates.notes')}
          </a>
        )}
        <ChromeButton size="sm" disabled={checking} onClick={() => refresh.mutate()}>
          {t('about.updates.check')}
        </ChromeButton>
      </div>
      <p className="mt-2 text-xs text-tx-3">{t('about.updates.cooldown')}</p>
    </div>
  );
}
