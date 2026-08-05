import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as teamsApi from '@/api/teams';
import type { Team } from '@/api/teams';
import * as usersApi from '@/api/users';
import { toApiError } from '@/lib/http';
import {
  restrictActionAccess,
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from './IamLayout';

export function Teams() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'iam.policies.manage',
  });
  const [editing, setEditing] = React.useState<Team | 'new' | null>(null);
  const [removing, setRemoving] = React.useState<Team | null>(null);

  const q = useQuery({ queryKey: ['iam', 'teams'], queryFn: () => teamsApi.list() });
  const usersQuery = useQuery({ queryKey: ['iam', 'users'], queryFn: () => usersApi.list() });
  const rows = q.data ?? [];

  // user id → display label, for rendering member chips.
  const userLabel = React.useMemo(() => {
    const map = new Map<string, string>();
    for (const u of usersQuery.data ?? []) map.set(u.id, u.display_name || u.email);
    return (id: string) => map.get(id) ?? id;
  }, [usersQuery.data]);

  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('teams.empty_title', { defaultValue: 'No teams yet' }),
    emptyDescription: t('teams.empty_description', {
      defaultValue: 'Group org members into teams to use as escalation targets.',
    }),
    emptyAction: (
      <ChromeButton
        variant="primary"
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onClick={() => manageAccess.allowed && setEditing('new')}
      >
        {t('teams.new', { defaultValue: 'New team' })}
      </ChromeButton>
    ),
  });

  const remove = useMutation({
    mutationFn: (id: string) => teamsApi.remove(id),
    onSuccess: () => {
      toast.success(t('teams.toast_deleted', { defaultValue: 'Team deleted' }));
      void qc.invalidateQueries({ queryKey: ['iam', 'teams'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <IamListPage
        title={t('teams.title', { defaultValue: 'Teams' })}
        subtitle={t('teams.subtitle', { defaultValue: 'Group members into teams for on-call and escalation.' }) as string}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => manageAccess.allowed && setEditing('new')}
          >
            {t('teams.new', { defaultValue: 'New team' })}
          </ChromeButton>
        }
        state={pageState}
      >
        <DataTable
          rows={rows}
          rowKey={(r) => r.id}
          onRowClick={(r) => manageAccess.allowed && setEditing(r)}
          isRowClickDisabled={() => manageAccess.disabled}
          rowClickDisabledReason={() => manageAccess.reason}
          columns={[
            {
              key: 'name',
              header: t('teams.columns.name', { defaultValue: 'Name' }),
              cell: (r) => <span className="font-strong text-tx-0">{r.name}</span>,
            },
            {
              key: 'members',
              header: t('teams.columns.members', { defaultValue: 'Members' }),
              cell: (r) =>
                r.member_ids.length === 0 ? (
                  <span className="text-tx-3">—</span>
                ) : (
                  <span className="flex flex-wrap gap-1">
                    {r.member_ids.slice(0, 4).map((id) => (
                      <Pill key={id} tone="indigo">
                        {userLabel(id)}
                      </Pill>
                    ))}
                    {r.member_ids.length > 4 && (
                      <Pill tone="dim">+{r.member_ids.length - 4}</Pill>
                    )}
                  </span>
                ),
            },
            {
              key: 'actions',
              header: '',
              width: 90,
              cell: (r) => {
                const deleteAccess = restrictActionAccess(
                  manageAccess,
                  !remove.isPending,
                  t('teams.action_pending'),
                );
                return (
                <ChromeButton
                  size="sm"
                  variant="ghost"
                  disabled={deleteAccess.disabled}
                  disabledReason={deleteAccess.reason}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (deleteAccess.allowed) setRemoving(r);
                  }}
                  className="text-red-soft enabled:hover:bg-red-dim"
                >
                  {t('teams.delete', { defaultValue: 'Delete' })}
                </ChromeButton>
                );
              },
            },
          ]}
        />
      </IamListPage>
      <TeamDrawer
        access={manageAccess}
        open={editing !== null}
        editing={editing === 'new' ? null : editing}
        users={usersQuery.data ?? []}
        onClose={() => setEditing(null)}
        onSaved={() => void qc.invalidateQueries({ queryKey: ['iam', 'teams'] })}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('teams.delete_confirm_title', { defaultValue: 'Delete team?' })}
        description={t('teams.delete_confirm_description', {
          defaultValue: 'This removes the team. Members keep their accounts.',
        })}
        confirmLabel={t('teams.delete', { defaultValue: 'Delete' })}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (manageAccess.allowed && removing) remove.mutate(removing.id);
        }}
      />
    </>
  );
}

function TeamDrawer({
  access,
  open,
  editing,
  users,
  onClose,
  onSaved,
}: {
  access: ActionAccess;
  open: boolean;
  editing: Team | null;
  users: usersApi.UserView[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation('iam');
  const isEdit = editing !== null;
  const [name, setName] = React.useState('');
  const [memberIds, setMemberIds] = React.useState<string[]>([]);

  React.useEffect(() => {
    setName(editing?.name ?? '');
    setMemberIds(editing?.member_ids ?? []);
  }, [editing]);

  const save = useMutation({
    mutationFn: () => {
      const payload: teamsApi.TeamInput = { name: name.trim(), member_ids: memberIds };
      return editing ? teamsApi.update(editing.id, payload) : teamsApi.create(payload);
    },
    onSuccess: () => {
      toast.success(t('teams.toast_saved', { defaultValue: 'Team “{{name}}” saved', name: name.trim() }));
      onSaved();
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const initialMemberIds = editing?.member_ids ?? [];
  const dirty =
    name.trim() !== (editing?.name ?? '') ||
    JSON.stringify([...memberIds].sort()) !==
      JSON.stringify([...initialMemberIds].sort());
  const invalid = !name.trim();
  const controlsDisabled = access.disabled || save.isPending;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={isEdit ? t('teams.edit_title', { defaultValue: 'Edit team' }) : t('teams.new', { defaultValue: 'New team' })}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={access.disabled || !dirty}
          disabledReason={
            access.reason ?? (!dirty ? t('teams.no_changes') : undefined)
          }
          invalid={invalid}
          onCancel={onClose}
          submitLabel={isEdit ? t('teams.save', { defaultValue: 'Save changes' }) : t('teams.create', { defaultValue: 'Create team' })}
          formId="team-form"
        />
      }
    >
      <form
        id="team-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (access.allowed && dirty && !invalid) save.mutate();
        }}
      >
        <FormSection title={t('teams.sections.identity', { defaultValue: 'Identity' })}>
          <FormField label={t('teams.fields.name', { defaultValue: 'Name' })} required>
            <FormInput
              value={name}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={(e) => setName(e.target.value)}
              placeholder="platform-eng"
              required
            />
          </FormField>
        </FormSection>
        <FormSection
          title={t('teams.sections.members', { defaultValue: 'Members' })}
          description={t('teams.members_description', { defaultValue: 'Pick the org members that belong to this team.' })}
        >
          {users.length === 0 ? (
            <p className="font-sans text-xs text-tx-3">
              {t('teams.no_users', { defaultValue: 'No users in this org yet.' })}
            </p>
          ) : (
            <FormChecklist
              options={users.map((u) => ({ value: u.id, label: u.display_name || u.email }))}
              selected={memberIds}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={setMemberIds}
            />
          )}
        </FormSection>
      </form>
    </FormDrawer>
  );
}
