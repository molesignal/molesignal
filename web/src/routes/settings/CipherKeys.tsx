import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as cipherApi from '@/api/cipherKeys';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { SectionBody } from './_atoms';
import { formatMicros } from '../rum/_helpers';

export function CipherKeys() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'org.settings.manage',
  });
  const [creating, setCreating] = React.useState(false);
  const [rotating, setRotating] = React.useState<cipherApi.CipherKey | null>(null);
  const [removing, setRemoving] = React.useState<cipherApi.CipherKey | null>(null);

  const q = useQuery({ queryKey: ['cipher-keys'], queryFn: () => cipherApi.list() });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('cipher_keys.empty_title'),
    emptyDescription: t('cipher_keys.empty_description'),
  });

  const remove = useMutation({
    mutationFn: (name: string) => cipherApi.remove(name),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void qc.invalidateQueries({ queryKey: ['cipher-keys'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader
        title={t('cipher_keys.title')}
        subtitle={t('cipher_keys.subtitle') as string}
        actions={
          <ChromeButton
            variant="primary"
            onClick={() => setCreating(true)}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            {t('cipher_keys.new_key')}
          </ChromeButton>
        }
      />
      <CreateDrawer
        open={creating}
        access={manageAccess}
        onClose={() => setCreating(false)}
      />
      <RotateDrawer
        keyMeta={rotating}
        access={manageAccess}
        onClose={() => setRotating(null)}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('cipher_keys.delete_confirm_title')}
        description={t('cipher_keys.delete_confirm_description')}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) {
            remove.mutate(removing.name);
          }
        }}
      />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.name}
            columns={[
              { key: 'name', header: t('cipher_keys.columns.name'), cell: (r) => r.name },
              { key: 'alg', header: t('cipher_keys.columns.alg'), cell: (r) => r.alg, width: 120 },
              {
                key: 'version',
                header: t('cipher_keys.columns.version'),
                cell: (r) => r.version,
                width: 90,
              },
              {
                key: 'rotated',
                header: t('cipher_keys.columns.rotated'),
                cell: (r) => formatMicros(r.rotated_at_micros),
                width: 200,
              },
              {
                key: 'actions',
                header: '',
                width: 180,
                cell: (r) => (
                  <div
                    className="flex justify-end gap-1"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={(e) => {
                        e.stopPropagation();
                        setRotating(r);
                      }}
                    >
                      {t('cipher_keys.rotate')}
                    </ChromeButton>
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={(e) => {
                        e.stopPropagation();
                        setRemoving(r);
                      }}
                      className="enabled:hover:text-red-soft"
                    >
                      {tc('actions.delete')}
                    </ChromeButton>
                  </div>
                ),
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}

function CreateDrawer({
  open,
  access,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [name, setName] = React.useState('');
  const [keyB64, setKeyB64] = React.useState('');

  React.useEffect(() => {
    if (!open) {
      setName('');
      setKeyB64('');
    }
  }, [open]);

  const create = useMutation({
    mutationFn: () => cipherApi.create({ name, key_material_b64: keyB64 }),
    onSuccess: () => {
      toast.success(t('cipher_keys.toast_created'));
      void qc.invalidateQueries({ queryKey: ['cipher-keys'] });
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const invalid = name.trim().length === 0 || keyB64.trim().length === 0;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={t('cipher_keys.create_drawer_title')}
      subtitle={t('cipher_keys.create_drawer_subtitle') as string}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          disabled={access.disabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid ? tc('access.form_invalid') : undefined)
          }
          onCancel={onClose}
          submitLabel={t('cipher_keys.new_key')}
          formId="cipher-create-form"
        />
      }
    >
      <form
        id="cipher-create-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (access.disabled || invalid || create.isPending) return;
          create.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('cipher_keys.field_name')} required>
            <FormInput
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={access.disabled || create.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField
            label={t('cipher_keys.field_key_material')}
            required
            hint={t('cipher_keys.field_key_material_hint')}
          >
            <FormTextarea
              value={keyB64}
              onChange={(e) => setKeyB64(e.target.value)}
              rows={4}
              className="font-sans text-xs"
              disabled={access.disabled || create.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

function RotateDrawer({
  keyMeta,
  access,
  onClose,
}: {
  keyMeta: cipherApi.CipherKey | null;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [keyB64, setKeyB64] = React.useState('');

  React.useEffect(() => {
    if (!keyMeta) setKeyB64('');
  }, [keyMeta]);

  const rotate = useMutation({
    mutationFn: () => {
      if (!keyMeta) throw new Error('no key');
      return cipherApi.rotate(keyMeta.name, { key_material_b64: keyB64 });
    },
    onSuccess: () => {
      toast.success(t('cipher_keys.toast_rotated'));
      void qc.invalidateQueries({ queryKey: ['cipher-keys'] });
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const invalid = keyB64.trim().length === 0;

  if (!keyMeta) return null;
  return (
    <FormDrawer
      open
      onOpenChange={(v) => !v && onClose()}
      title={t('cipher_keys.rotate_drawer_title', { name: keyMeta.name })}
      subtitle={t('cipher_keys.rotate_drawer_subtitle') as string}
      footer={
        <FormSubmitFooter
          busy={rotate.isPending}
          disabled={access.disabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid ? tc('access.form_invalid') : undefined)
          }
          onCancel={onClose}
          submitLabel={t('cipher_keys.rotate')}
          formId="cipher-rotate-form"
        />
      }
    >
      <form
        id="cipher-rotate-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (access.disabled || invalid || rotate.isPending) return;
          rotate.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('cipher_keys.field_key_material')} required>
            <FormTextarea
              value={keyB64}
              onChange={(e) => setKeyB64(e.target.value)}
              rows={4}
              className="font-sans text-xs"
              disabled={access.disabled || rotate.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
