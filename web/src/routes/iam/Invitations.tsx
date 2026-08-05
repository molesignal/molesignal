import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, RefreshCw, XCircle } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as invitationsApi from '@/api/invitations';
import * as rolesApi from '@/api/roles';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from './IamLayout';
import { formatMicros } from '../rum/_helpers';

export function Invitations() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const { t: tc } = useTranslation('common');
  const manageAccess = useActionAccess({
    permission: 'org.members.manage',
  });
  const [open, setOpen] = React.useState(false);

  const invitationsQuery = useQuery({
    queryKey: ['iam', 'invitations'],
    queryFn: () => invitationsApi.list(),
  });

  const refresh = () => qc.invalidateQueries({ queryKey: ['iam', 'invitations'] });

  const resend = useMutation({
    mutationFn: (id: string) => invitationsApi.resend(id),
    onSuccess: () => {
      toast.success(t('invitations.toast_resent'));
      void refresh();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const revoke = useMutation({
    mutationFn: (id: string) => invitationsApi.revoke(id),
    onSuccess: () => {
      toast.success(t('invitations.toast_revoked'));
      void refresh();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const state = productStateFor(queryStateFor({
    isLoading: invitationsQuery.isLoading,
    isError: invitationsQuery.isError,
    data: invitationsQuery.data ?? [],
  }), {
    error: invitationsQuery.error,
    emptyTitle: t('invitations.empty_title'),
    emptyDescription: t('invitations.empty_description'),
    emptyAction: (
      <ChromeButton
        variant="primary"
        onClick={() => setOpen(true)}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
      >
        <Plus className="h-3 w-3" /> {t('invitations.invite')}
      </ChromeButton>
    ),
  });

  return (
    <>
      <IamListPage
        title={t('invitations.title')}
        toolbar={
          <ChromeButton
            variant="primary"
            onClick={() => setOpen(true)}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            <Plus className="h-3 w-3" /> {t('invitations.invite')}
          </ChromeButton>
        }
        state={state}
      >
        {!state && (
          <DataTable
            rows={invitationsQuery.data ?? []}
            rowKey={(row) => row.id}
            columns={[
              {
                key: 'email',
                header: t('invitations.columns.email'),
                cell: (row) => <span className="text-tx-0">{row.email}</span>,
              },
              {
                key: 'role',
                header: t('invitations.columns.role'),
                cell: (row) => row.role_name,
                width: 120,
              },
              {
                key: 'status',
                header: t('invitations.columns.status'),
                cell: (row) => (
                  <Pill tone={row.status === 'pending' ? 'yellow' : row.status === 'revoked' ? 'red' : 'green'}>
                    {row.status}
                  </Pill>
                ),
                width: 120,
              },
              {
                key: 'sent',
                header: t('invitations.columns.sent'),
                cell: (row) => formatMicros(row.sent_at_micros),
                width: 180,
              },
              {
                key: 'inviter',
                header: t('invitations.columns.inviter'),
                cell: (row) => row.inviter_id,
                width: 220,
              },
              {
                key: 'actions',
                header: t('invitations.columns.actions'),
                width: 180,
                cell: (row) => (
                  <div
                    className="flex items-center gap-1"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <IconButton
                      disabled={
                        manageAccess.disabled ||
                        row.status === 'accepted' ||
                        resend.isPending
                      }
                      disabledReason={
                        manageAccess.reason ??
                        (row.status === 'accepted'
                          ? t('invitations.completed_reason')
                          : resend.isPending
                            ? tc('access.operation_pending')
                            : undefined)
                      }
                      onClick={() => resend.mutate(row.id)}
                      aria-label={t('invitations.resend')}
                    >
                      <RefreshCw className="h-3 w-3" />
                    </IconButton>
                    <IconButton
                      disabled={
                        manageAccess.disabled ||
                        row.status !== 'pending' ||
                        revoke.isPending
                      }
                      disabledReason={
                        manageAccess.reason ??
                        (row.status !== 'pending'
                          ? t('invitations.not_pending_reason')
                          : revoke.isPending
                            ? tc('access.operation_pending')
                            : undefined)
                      }
                      onClick={() => revoke.mutate(row.id)}
                      aria-label={t('invitations.revoke')}
                      className="enabled:hover:bg-red-dim enabled:hover:text-red-soft"
                    >
                      <XCircle className="h-3 w-3" />
                    </IconButton>
                  </div>
                ),
              },
            ]}
          />
        )}
      </IamListPage>
      <InviteDrawer
        open={open}
        access={manageAccess}
        onClose={() => setOpen(false)}
      />
    </>
  );
}

function InviteDrawer({
  open,
  access,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [email, setEmail] = React.useState('');
  const [roleId, setRoleId] = React.useState('');
  const rolesQuery = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: () => rolesApi.list(),
    enabled: open,
  });

  const create = useMutation({
    mutationFn: () =>
      invitationsApi.create({
        email,
        ...(roleId ? { role_id: roleId } : {}),
      }),
    onSuccess: () => {
      toast.success(t('invitations.toast_invited'));
      void qc.invalidateQueries({ queryKey: ['iam', 'invitations'] });
      setEmail('');
      setRoleId('');
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const invalid = !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email.trim());
  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (access.disabled || invalid || create.isPending) return;
    create.mutate();
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('invitations.drawer_title')}
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
          submitLabel={t('invitations.submit_label')}
          formId="invite-form"
        />
      }
    >
      <form id="invite-form" onSubmit={submit}>
        <FormSection>
          <FormField label={t('invitations.fields.email')} required>
            <FormInput
              type="email"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              placeholder="teammate@example.com"
              disabled={access.disabled || create.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField label={t('invitations.fields.role')} required>
            <FormSelect
              value={roleId}
              onChange={setRoleId}
              disabled={access.disabled || create.isPending}
              disabledReason={access.reason}
              options={[
                { value: '', label: t('invitations.fields.default_role') },
                ...(rolesQuery.data ?? []).map((role) => ({
                  value: role.id,
                  label: role.name,
                })),
              ]}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
