import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as orgsApi from '@/api/orgs';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { useOrgStore } from '@/stores/useOrgStore';

import { IamListPage } from './IamLayout';

export function Organizations() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const navigate = useNavigate();
  const manageAccess = useActionAccess({
    permission: 'sys.organizations.manage',
  });
  const [drawerOpen, setDrawerOpen] = React.useState(false);
  const currentOrgId = useOrgStore((s) => s.currentOrgId);
  const loadOrgs = useOrgStore((s) => s.loadOrgs);
  const setOrgs = useOrgStore((s) => s.setOrgs);
  const switchOrg = useOrgStore((s) => s.switchOrg);
  const upsertOrg = useOrgStore((s) => s.upsertOrg);
  const [switchingOrgId, setSwitchingOrgId] = React.useState<string | null>(null);
  const q = useQuery({ queryKey: ['iam', 'orgs'], queryFn: () => orgsApi.listOrgs() });
  const rows = q.data ?? [];
  React.useEffect(() => {
    if (q.data) setOrgs(q.data);
  }, [q.data, setOrgs]);

  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('organizations.empty_title'),
    emptyDescription: t('organizations.empty_description'),
  });

  const openOrganizationIam = async (organization: orgsApi.Org) => {
    setSwitchingOrgId(organization.id);
    try {
      await switchOrg(organization.id, { queryClient: qc });
      toast.success(
        t('organizations.toast_opened_iam', { name: organization.name }),
      );
      navigate('/iam/users');
    } catch (error) {
      toast.error(toApiError(error).message);
    } finally {
      setSwitchingOrgId(null);
    }
  };

  return (
    <IamListPage
      title={t('organizations.title')}
      subtitle={t('organizations.system_subtitle')}
      toolbar={
        <ChromeButton
          variant="primary"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onClick={() => {
            if (manageAccess.allowed) setDrawerOpen(true);
          }}
        >
          <Plus className="h-3 w-3" /> {t('organizations.create')}
        </ChromeButton>
      }
      state={pageState}
    >
      <DataTable
        rows={rows}
        rowKey={(r) => r.id}
        columns={[
          {
            key: 'name',
            header: t('organizations.columns.name'),
            cell: (r) => (
              <span className="flex items-center gap-2 text-tx-0">
                {r.name}
                {r.id === currentOrgId && <Pill tone="orange">{t('organizations.current_badge')}</Pill>}
              </span>
            ),
          },
          {
            key: 'role',
            header: t('organizations.columns.role'),
            cell: (r) =>
              r.display_role ?? t('organizations.not_a_member'),
            width: 120,
          },
          { key: 'slug', header: t('organizations.columns.slug'), cell: (r) => r.slug ?? '—' },
          {
            key: 'id',
            header: t('organizations.columns.id'),
            cell: (r) => <span className="font-sans text-tx-3">{r.id}</span>,
          },
          {
            key: 'actions',
            header: t('organizations.columns.actions'),
            width: 140,
            cell: (r) => {
              const switching = switchingOrgId !== null;
              const disabled =
                switching ||
                Boolean(r.system) ||
                r.disabled ||
                r.roles.length === 0;
              const disabledReason = switching
                ? t('organizations.switching')
                : r.system
                  ? t('organizations.system_iam_unavailable')
                  : r.disabled
                    ? t('organizations.disabled_iam_unavailable')
                    : r.roles.length === 0
                      ? t('organizations.membership_required')
                      : undefined;
              return (
                <div className="flex justify-end">
                  <ChromeButton
                    variant="ghost"
                    size="sm"
                    disabled={disabled}
                    disabledReason={disabledReason}
                    onClick={() => void openOrganizationIam(r)}
                  >
                    {switchingOrgId === r.id
                      ? t('organizations.opening_iam')
                      : t('organizations.manage_iam')}
                  </ChromeButton>
                </div>
              );
            },
          },
        ]}
      />
      <CreateOrgDrawer
        access={manageAccess}
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        onCreated={(org) => {
          upsertOrg(org);
          qc.setQueryData<orgsApi.Org[]>(['iam', 'orgs'], (prev) => mergeOrg(prev, org));
          qc.setQueryData<orgsApi.Org[]>(['orgs', 'list'], (prev) => mergeOrg(prev, org));
          void qc.invalidateQueries({ queryKey: ['iam', 'orgs'] });
          void loadOrgs();
        }}
      />
    </IamListPage>
  );
}

function CreateOrgDrawer({
  access,
  open,
  onClose,
  onCreated,
}: {
  access: ActionAccess;
  open: boolean;
  onClose: () => void;
  onCreated: (org: orgsApi.Org) => void;
}) {
  const { t } = useTranslation('iam');
  const [name, setName] = React.useState('');
  const [slug, setSlug] = React.useState('');
  const [slugTouched, setSlugTouched] = React.useState(false);

  React.useEffect(() => {
    if (!open) return;
    setName('');
    setSlug('');
    setSlugTouched(false);
  }, [open]);

  const create = useMutation({
    mutationFn: () => orgsApi.createOrg({ name: name.trim(), slug: slug.trim() }),
    onSuccess: (org) => {
      toast.success(t('organizations.toast_created'));
      onCreated(org);
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (access.allowed) create.mutate();
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(value) => !value && onClose()}
      title={t('organizations.create')}
      subtitle={t('organizations.drawer_subtitle')}
      width={520}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          disabled={access.disabled}
          invalid={!name.trim() || !slug.trim()}
          disabledReason={access.reason}
          onCancel={onClose}
          submitLabel={t('organizations.create')}
          formId="create-org-form"
        />
      }
    >
      <form id="create-org-form" onSubmit={submit}>
        <FormSection title={t('organizations.sections.identity')}>
          <FormRow>
            <FormField label={t('organizations.fields.name')} required>
              <FormInput
                value={name}
                onChange={(event) => {
                  const next = event.target.value;
                  setName(next);
                  if (!slugTouched) setSlug(slugify(next));
                }}
                placeholder={t('organizations.fields.name_placeholder')}
                disabled={access.disabled || create.isPending}
                disabledReason={access.reason}
                required
              />
            </FormField>
            <FormField label={t('organizations.fields.slug')} required>
              <FormInput
                value={slug}
                onChange={(event) => {
                  setSlugTouched(true);
                  setSlug(slugify(event.target.value));
                }}
                placeholder="example-production"
                disabled={access.disabled || create.isPending}
                disabledReason={access.reason}
                required
              />
            </FormField>
          </FormRow>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

function mergeOrg(prev: orgsApi.Org[] | undefined, org: orgsApi.Org): orgsApi.Org[] {
  return [...(prev ?? []).filter((item) => item.id !== org.id), org].sort((a, b) =>
    a.name.localeCompare(b.name),
  );
}

function slugify(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48);
}
