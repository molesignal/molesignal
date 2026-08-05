import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { FileCode2, Upload } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as debugArtifactsApi from '@/api/debugArtifacts';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill } from '@/shell/chrome';
import { ErrorState } from '@/shell/ErrorState';
import { LoadingState } from '@/shell/LoadingState';
import { toast } from '@/shell/ui/sonner';

import { formatMicros } from '../_helpers';
import { rumDocumentationUrl } from '../documentation';
import { RumListPage, RumSectionHeader, useRumBasePath } from '../RumLayout';

export function SourceMaps() {
  const { t, i18n } = useTranslation('rum');
  const navigate = useNavigate();
  const basePath = useRumBasePath();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'streams.configure',
  });
  const [deleting, setDeleting] = React.useState<debugArtifactsApi.DebugArtifactMeta | null>(null);

  const query = useQuery({
    queryKey: ['rum', 'debug-artifacts'],
    queryFn: () => debugArtifactsApi.list(),
  });
  const rows = query.data ?? [];
  const uploadAction = (
    <ChromeButton
      variant="primary"
      disabled={manageAccess.disabled}
      disabledReason={manageAccess.reason}
      onClick={() =>
        manageAccess.allowed &&
        navigate(`${basePath}/settings/source-maps/upload`)
      }
    >
      <Upload className="h-4 w-4" />
      {t('source_maps.upload_cta')}
    </ChromeButton>
  );

  const remove = useMutation({
    mutationFn: (id: string) => debugArtifactsApi.remove(id),
    onSuccess: () => {
      toast.success(t('source_maps.toast_deleted'));
      void queryClient.invalidateQueries({ queryKey: ['rum', 'debug-artifacts'] });
      setDeleting(null);
    },
  });

  return (
    <>
      <RumListPage
        title={t('source_maps.title')}
        subtitle={t('source_maps.subtitle') as string}
        toolbar={uploadAction}
        settings
      >
        {query.isLoading ? (
          <LoadingState variant="list" rows={5} />
        ) : query.isError ? (
          <ErrorState
            error={query.error}
            title={t('source_maps.load_error')}
            onRetry={() => void query.refetch()}
          />
        ) : rows.length === 0 ? (
          <SourceMapOnboarding
            uploadDisabled={manageAccess.disabled}
            uploadDisabledReason={manageAccess.reason}
            onUpload={() => {
              if (manageAccess.allowed) {
                navigate(`${basePath}/settings/source-maps/upload`);
              }
            }}
            docsHref={rumDocumentationUrl(
              i18n.resolvedLanguage ?? i18n.language,
              'source-maps',
            )}
          />
        ) : (
          <section>
            <RumSectionHeader
              title={t('source_maps.active_maps')}
              description={t('source_maps.active_maps_description', { count: rows.length })}
            />
            <div className="border-b border-bd-0">
              <div className="hidden min-h-10 grid-cols-[130px_130px_110px_170px_90px_minmax(180px,1fr)_160px_72px] items-center gap-4 border-b border-bd-0 text-xs font-strong text-tx-3 lg:grid">
                <span>{t('source_maps.columns.application')}</span>
                <span>{t('source_maps.columns.service')}</span>
                <span>{t('source_maps.columns.release')}</span>
                <span>{t('source_maps.columns.kind')}</span>
                <span>{t('source_maps.columns.platform')}</span>
                <span>{t('source_maps.columns.filename')}</span>
                <span>{t('source_maps.columns.uploaded')}</span>
                <span />
              </div>
              <div className="divide-y divide-bd-0">
                {rows.map((row) => (
                  <div
                    key={row.id}
                    className="grid gap-3 py-4 lg:grid-cols-[130px_130px_110px_170px_90px_minmax(180px,1fr)_160px_72px] lg:items-center lg:gap-4"
                  >
                    <span className="truncate font-mono text-xs text-tx-1">{row.application_id}</span>
                    <span className="truncate text-sm font-strong text-tx-0">{row.service}</span>
                    <span className="truncate font-mono text-xs text-tx-1">{row.release}</span>
                    <Pill tone="blue">{t(`source_maps.kinds.${row.kind}`)}</Pill>
                    <span className="min-w-0 text-xs font-strong uppercase text-tx-2">
                      <span className="block truncate">{row.platform}</span>
                      {row.architecture && (
                        <span className="mt-0.5 block truncate font-mono font-normal normal-case text-tx-3">
                          {row.architecture}
                        </span>
                      )}
                    </span>
                    <span className="flex min-w-0 items-start gap-2">
                      <FileCode2 className="h-4 w-4 shrink-0 text-blue-soft" />
                      <span className="min-w-0">
                        <span className="block truncate font-mono text-xs text-tx-1">
                          {row.filename}
                        </span>
                        {row.debug_id && (
                          <span className="mt-0.5 block truncate font-mono text-xs text-tx-3">
                            {row.debug_id}
                          </span>
                        )}
                      </span>
                    </span>
                    <span className="text-xs text-tx-2">{formatMicros(row.uploaded_at_micros)}</span>
                    <ChromeButton
                      size="sm"
                      variant="ghost"
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={() => manageAccess.allowed && setDeleting(row)}
                      className="justify-self-start text-red-soft enabled:hover:bg-red-dim lg:justify-self-end"
                    >
                      {t('source_maps.delete')}
                    </ChromeButton>
                  </div>
                ))}
              </div>
            </div>
          </section>
        )}
      </RumListPage>

      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('source_maps.delete_confirm_title')}
        description={t('source_maps.delete_confirm_description')}
        destructive
        confirmLabel={t('source_maps.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (manageAccess.allowed && deleting) remove.mutate(deleting.id);
        }}
      />
    </>
  );
}

function SourceMapOnboarding({
  uploadDisabled,
  uploadDisabledReason,
  onUpload,
  docsHref,
}: {
  uploadDisabled: boolean;
  uploadDisabledReason?: string | undefined;
  onUpload: () => void;
  docsHref: string;
}) {
  const { t } = useTranslation('rum');
  return (
    <section className="mx-auto grid max-w-5xl gap-8 py-10 lg:grid-cols-[minmax(0,1fr)_minmax(320px,.8fr)] lg:items-center">
      <div>
        <span className="grid h-12 w-12 place-items-center rounded-lg bg-indigo-dim text-indigo-soft">
          <FileCode2 className="h-6 w-6" />
        </span>
        <h2 className="mb-0 mt-5 text-2xl font-display-strong tracking-[-0.02em] text-tx-0">
          {t('source_maps.onboarding_title')}
        </h2>
        <p className="mb-0 mt-3 max-w-xl text-sm leading-relaxed text-tx-2">
          {t('source_maps.onboarding_description')}
        </p>
        <div className="mt-6 flex flex-wrap gap-3">
          <ChromeButton
            variant="primary"
            disabled={uploadDisabled}
            disabledReason={uploadDisabledReason}
            onClick={onUpload}
          >
            <Upload className="h-4 w-4" />
            {t('source_maps.upload_cta')}
          </ChromeButton>
          <a
            href={docsHref}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex h-9 shrink-0 items-center gap-2 whitespace-nowrap rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm font-strong text-tx-1 transition-colors duration-fast ease-default hover:border-bd-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3"
          >
            {t('source_maps.view_docs')}
          </a>
        </div>
      </div>
      <div className="overflow-hidden rounded-lg border border-bd-1 bg-bg-1">
        <div className="border-b border-bd-0 px-4 py-3 text-xs font-strong text-tx-3">
          {t('source_maps.stack_example')}
        </div>
        <div className="grid gap-4 p-5">
          <div>
            <span className="text-xs font-strong text-red-soft">{t('source_maps.before')}</span>
            <code className="mt-2 block rounded-md bg-red-dim p-3 font-mono text-xs text-red-soft">
              app.min.js:1:38291
            </code>
          </div>
          <div className="flex items-center gap-3 text-xs font-strong text-tx-3">
            <span className="h-px flex-1 bg-bd-0" />
            {t('source_maps.demangle')}
            <span className="h-px flex-1 bg-bd-0" />
          </div>
          <div>
            <span className="text-xs font-strong text-green-soft">{t('source_maps.after')}</span>
            <code className="mt-2 block rounded-md bg-green-dim p-3 font-mono text-xs text-green-soft">
              src/checkout/payment.ts:86
            </code>
          </div>
        </div>
      </div>
    </section>
  );
}
