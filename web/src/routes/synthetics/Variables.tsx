import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { KeyRound, Plus, RefreshCw, RotateCw, ShieldCheck } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable, type DataTableColumn } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { SyntheticSecret } from '@/api/synthetics';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { Section, SyntheticsPage, WorkspaceBoundary } from './components';
import { formatRelativeTimestamp } from './model';

const RUNTIME_VARIABLES = [
  ['timestamp', 'variables.runtime_timestamp'],
  ['uuid', 'variables.runtime_uuid'],
  ['random', 'variables.runtime_random'],
  ['location', 'variables.runtime_location'],
  ['previous_response', 'variables.runtime_previous_response'],
] as const;

export function Variables() {
  const { t, i18n } = useTranslation('synthetics');
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const canManage = hasPermission('synthetics.secrets.manage', access);
  const [createOpen, setCreateOpen] = React.useState(false);
  const [rotating, setRotating] = React.useState<SyntheticSecret>();
  const secretsQuery = useQuery({ queryKey: ['synthetics', 'secrets'], queryFn: syntheticsApi.listSecrets });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['synthetics', 'secrets'] });
  return (
    <SyntheticsPage
      title={t('variables.title')}
      subtitle={t('variables.subtitle')}
      toolbar={
        <div className="flex items-center gap-2">
          <ChromeButton onClick={() => void secretsQuery.refetch()}>
            <RefreshCw aria-hidden className="h-3.5 w-3.5" />
            {t('actions.refresh')}
          </ChromeButton>
          {canManage && <ChromeButton variant="primary" onClick={() => setCreateOpen(true)}><Plus className="h-3.5 w-3.5" />{t('actions.create_secret')}</ChromeButton>}
        </div>
      }
    >
      <Section title={t('variables.runtime_title')} description={t('variables.runtime_hint')}>
        <div className="grid gap-3 p-4 sm:grid-cols-2 xl:grid-cols-3">
          {RUNTIME_VARIABLES.map(([name, description]) => (
            <div key={name} className="flex min-h-20 items-center gap-3 rounded-md border border-bd-0 bg-bg-2 p-3">
              <span className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-indigo/10 text-indigo-soft"><KeyRound className="h-4 w-4" /></span>
              <div className="min-w-0 flex-1"><code className="font-code text-xs font-strong text-tx-0">{`{{${name}}}`}</code><p className="mt-1 text-xs text-tx-3">{t(description)}</p></div>
              <CopyVariable value={`{{${name}}}`} />
            </div>
          ))}
        </div>
      </Section>

      <Section title={t('variables.secrets_title')} description={t('variables.secret_hint')}>
        <WorkspaceBoundary pending={secretsQuery.isPending} error={secretsQuery.error} onRetry={() => void secretsQuery.refetch()}>
          <DataTable
            rows={secretsQuery.data ?? []}
            columns={secretColumns(i18n.language, t, canManage, setRotating)}
            rowKey={(secret) => secret.id}
            emptyLabel={t('states.no_secrets')}
          />
        </WorkspaceBoundary>
      </Section>

      <SecretDrawer open={createOpen} onOpenChange={setCreateOpen} onSaved={refresh} />
      <SecretDrawer open={Boolean(rotating)} onOpenChange={(open) => !open && setRotating(undefined)} secret={rotating} onSaved={async () => { setRotating(undefined); await refresh(); }} />
    </SyntheticsPage>
  );
}

function secretColumns(locale: string, t: TFunction<'synthetics'>, canManage: boolean, onRotate: (secret: SyntheticSecret) => void): DataTableColumn<SyntheticSecret>[] {
  return [
    { key: 'name', header: t('variables.name'), width: 240, cell: (secret) => <div className="flex items-center gap-2"><ShieldCheck className="h-4 w-4 text-green" /><div><div className="font-code text-xs font-strong text-tx-0">{secret.name}</div><div className="mt-0.5 text-type-micro text-tx-3">{secret.description}</div></div></div> },
    { key: 'version', header: t('variables.version'), cell: (secret) => `v${secret.current_version}` },
    { key: 'updated', header: t('variables.updated'), cell: (secret) => formatRelativeTimestamp(secret.updated_at, locale) },
    { key: 'usage', header: t('variables.reference'), cell: (secret) => <code className="font-code text-xs text-indigo-soft">{`{{${secret.name}}}`}</code> },
    { key: 'actions', header: <span className="sr-only">{t('checks.columns.actions')}</span>, width: 60, cell: (secret) => <Tooltip><TooltipTrigger asChild><IconButton aria-label={t('actions.rotate')} disabled={!canManage} onClick={() => onRotate(secret)}><RotateCw className="h-3.5 w-3.5" /></IconButton></TooltipTrigger><TooltipContent>{t('actions.rotate')}</TooltipContent></Tooltip> },
  ];
}

function SecretDrawer({ open, onOpenChange, secret, onSaved }: { open: boolean; onOpenChange: (open: boolean) => void; secret?: SyntheticSecret | undefined; onSaved: () => void | Promise<void> }) {
  const { t } = useTranslation('synthetics');
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [value, setValue] = React.useState('');
  React.useEffect(() => { if (open) { setName(secret?.name ?? ''); setDescription(secret?.description ?? ''); setValue(''); } }, [open, secret]);
  const save = useMutation({
    mutationFn: () => secret ? syntheticsApi.rotateSecret(secret.id, value) : syntheticsApi.createSecret({ name: name.trim(), description: description.trim(), value }),
    onSuccess: async () => { toast.success(t(secret ? 'variables.rotated' : 'variables.created')); onOpenChange(false); await onSaved(); },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const invalid = (!secret && !name.trim()) || !value;
  return <FormDrawer open={open} onOpenChange={onOpenChange} title={secret ? t('variables.rotate_title', { name: secret.name }) : t('variables.create_title')} subtitle={t('variables.secret_hint')} footer={<FormSubmitFooter busy={save.isPending} invalid={invalid} onCancel={() => onOpenChange(false)} formId="synthetic-secret-form" submitLabel={secret ? t('actions.rotate') : t('actions.create_secret')} />}><form id="synthetic-secret-form" onSubmit={(event) => { event.preventDefault(); if (!invalid) save.mutate(); }}><FormSection>{!secret && <><FormField label={t('variables.name')} required><FormInput value={name} onChange={(event) => setName(event.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, '_'))} placeholder="API_KEY" /></FormField><FormField label={t('variables.description')}><FormTextarea value={description} onChange={(event) => setDescription(event.target.value)} /></FormField></>}<FormField label={t('variables.value')} required><FormInput type="password" autoComplete="new-password" value={value} onChange={(event) => setValue(event.target.value)} /></FormField></FormSection></form></FormDrawer>;
}

function CopyVariable({ value }: { value: string }) {
  const [copied, setCopied] = React.useState(false);
  const { t } = useTranslation('synthetics');
  return <CopyIconButton label={t('actions.copy_value', { value })} copied={copied} onClick={() => void writeClipboardText(value).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1500); })} tooltipSide="left" />;
}
