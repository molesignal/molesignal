import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, RotateCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { SyntheticSecret } from '@/api/synthetics';
import { writeClipboardText } from '@/lib/clipboard';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { formatRelativeMicros } from '@/time/relative';

import { SyntheticsPage, SyntheticsSection, useLoadState } from '../Frame';
import { SecretDrawer } from './SecretDrawer';

const RUNTIME_VARIABLES = ['timestamp', 'uuid', 'random', 'location', 'previous_response'] as const;

/** How a check refers to a variable or a secret: the name in double braces. */
function reference(name: string): string {
  return `{{${name}}}`;
}

/**
 * What a check can be given at run time: the secrets an organization keeps
 * here, and the values the probe works out for itself.
 *
 * Secrets come first, since they are what a person manages; the built-in
 * variables are a reference to copy from. Both are used the same way, so each
 * row shows the reference to paste and a way to copy it.
 */
export function Variables() {
  const { t, i18n } = useTranslation('synthetics');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'synthetics.secrets.manage' });
  const [createOpen, setCreateOpen] = React.useState(false);
  const [rotating, setRotating] = React.useState<SyntheticSecret>();
  const secretsQuery = useQuery({
    queryKey: ['synthetics', 'secrets'],
    queryFn: syntheticsApi.listSecrets,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['synthetics', 'secrets'] });
  const state = useLoadState({
    pending: secretsQuery.isPending,
    error: secretsQuery.error,
    onRetry: () => void secretsQuery.refetch(),
  });
  const nowMicros = Date.now() * 1000;

  return (
    <>
      <SyntheticsPage
        state={state}
        contentSurface={false}
        action={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => setCreateOpen(true)}
          >
            <Plus aria-hidden className="h-3.5 w-3.5" />
            {t('actions.create_secret')}
          </ChromeButton>
        }
      >
        <SyntheticsSection
          title={t('variables.secrets_title')}
          description={t('variables.secret_hint')}
        >
          <div className="overflow-x-auto border-t border-bd-0">
            <DataTable
              rows={secretsQuery.data ?? []}
              rowKey={(secret) => secret.id}
              emptyLabel={t('states.no_secrets')}
              className="min-w-[720px] rounded-none border-0 bg-transparent"
              columns={[
                {
                  key: 'name',
                  header: t('variables.name'),
                  cell: (secret) => (
                    <div className="min-w-0 py-1">
                      <div className="truncate font-code text-xs font-strong text-tx-0">
                        {secret.name}
                      </div>
                      {secret.description && (
                        <div className="mt-0.5 truncate text-xs text-tx-2">{secret.description}</div>
                      )}
                    </div>
                  ),
                },
                {
                  key: 'reference',
                  header: t('variables.reference'),
                  width: 280,
                  cell: (secret) => <Reference name={secret.name} />,
                },
                {
                  key: 'version',
                  header: t('variables.version'),
                  width: 100,
                  cell: (secret) => (
                    <span className="tabular-nums text-tx-1">v{secret.current_version}</span>
                  ),
                },
                {
                  key: 'updated',
                  header: t('variables.updated'),
                  width: 150,
                  cell: (secret) => (
                    <span className="text-tx-2">
                      {formatRelativeMicros(secret.updated_at, i18n.language, nowMicros)}
                    </span>
                  ),
                },
                {
                  key: 'actions',
                  header: <span className="sr-only">{t('checks.columns.actions')}</span>,
                  width: 56,
                  className: 'overflow-visible text-right',
                  cell: (secret) => (
                    <div className="flex justify-end">
                      <IconButton
                        aria-label={t('variables.rotate_title', { name: secret.name })}
                        title={t('actions.rotate')}
                        disabled={manageAccess.disabled}
                        disabledReason={manageAccess.reason}
                        onClick={() => setRotating(secret)}
                      >
                        <RotateCw className="h-3.5 w-3.5" />
                      </IconButton>
                    </div>
                  ),
                },
              ]}
            />
          </div>
        </SyntheticsSection>

        <SyntheticsSection
          title={t('variables.runtime_title')}
          description={t('variables.runtime_hint')}
        >
          <ul className="divide-y divide-bd-0 border-t border-bd-0">
            {RUNTIME_VARIABLES.map((name) => (
              <li
                key={name}
                className="grid min-h-[44px] grid-cols-1 items-center gap-x-[16px] gap-y-[2px] px-4 py-[8px] sm:grid-cols-[minmax(0,280px)_minmax(0,1fr)]"
              >
                <Reference name={name} />
                <span className="min-w-0 font-sans text-sm text-tx-1">
                  {t(`variables.runtime_${name}`)}
                </span>
              </li>
            ))}
          </ul>
        </SyntheticsSection>
      </SyntheticsPage>

      <SecretDrawer open={createOpen} onOpenChange={setCreateOpen} onSaved={refresh} />
      <SecretDrawer
        open={Boolean(rotating)}
        onOpenChange={(open) => !open && setRotating(undefined)}
        secret={rotating}
        onSaved={async () => {
          setRotating(undefined);
          await refresh();
        }}
      />
    </>
  );
}

/** The text a check is written with, and a way to take it. */
function Reference({ name }: { name: string }) {
  const { t } = useTranslation('synthetics');
  const [copied, setCopied] = React.useState(false);
  const value = reference(name);

  return (
    <span className="inline-flex min-w-0 items-center gap-[4px]">
      <code className="min-w-0 truncate font-code text-xs text-tx-0">{value}</code>
      <CopyIconButton
        label={t('actions.copy_value', { value })}
        copied={copied}
        onClick={() =>
          void writeClipboardText(value).then(() => {
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1500);
          })
        }
        tooltipSide="right"
      />
    </span>
  );
}
