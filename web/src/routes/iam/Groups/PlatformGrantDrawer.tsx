import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as grantsApi from '@/api/groups';
import type * as orgsApi from '@/api/orgs';
import type * as rolesApi from '@/api/roles';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import { isOrganizationNarrowable } from '@/product/permissions';
import { DateTimePicker } from '@/shell/DateTimePicker';
import {
  FormDrawer,
  FormField,
  FormRow,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/shell/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { toast } from '@/shell/ui/sonner';

import { invalidateIamAccess } from './shared';

const WHOLE_PLATFORM = '';

export function PlatformGrantDrawer({
  access,
  organizations,
  roles,
  open,
  onClose,
}: {
  access: ActionAccess;
  organizations: readonly orgsApi.Org[];
  roles: readonly rolesApi.IamRole[];
  open: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const [account, setAccount] = React.useState<grantsApi.PlatformAccount | null>(null);
  const [roleId, setRoleId] = React.useState('');
  const [organizationId, setOrganizationId] = React.useState(WHOLE_PLATFORM);
  const [expiresAt, setExpiresAt] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    setAccount(null);
    setRoleId('');
    setOrganizationId(WHOLE_PLATFORM);
    setExpiresAt('');
  }, [open]);

  const role = roles.find((candidate) => candidate.id === roleId);
  const narrowable = Boolean(role?.effectivePermissions.some(isOrganizationNarrowable));
  const tenants = organizations.filter((organization) => !organization.system);
  const narrowed = narrowable && organizationId !== WHOLE_PLATFORM;

  const save = useMutation({
    mutationFn: () =>
      grantsApi.createRoleBinding({
        roleId: roleId,
        principalType: 'user',
        principalId: account?.id ?? '',
        ...(narrowed ? { resourceType: 'organization', resourceId: organizationId } : {}),
        ...(expiresAt ? { expiresAt: new Date(expiresAt).getTime() * 1_000 } : {}),
      }),
    onSuccess: async () => {
      toast.success(t('groups.toast_created'));
      onClose();
      await invalidateIamAccess(qc);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const invalid = !account || !roleId;
  const controlsDisabled = access.disabled || save.isPending;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('groups.new_grant')}
      subtitle={t('groups.platform.drawer_subtitle')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={access.disabled}
          disabledReason={access.reason}
          invalid={invalid}
          onCancel={onClose}
          submitLabel={t('groups.create_grant')}
          formId="platform-grant-form"
        />
      }
    >
      <form
        id="platform-grant-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (access.allowed && !invalid) save.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('groups.platform.columns.account')} required>
            <AccountCombobox value={account} onChange={setAccount} disabled={controlsDisabled} />
          </FormField>
          <FormField label={t('groups.fields.role')} required>
            <FormSelect
              value={roleId}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={(next) => {
                setRoleId(next);
                setOrganizationId(WHOLE_PLATFORM);
              }}
              options={[
                { value: '', label: t('groups.fields.select_role') },
                ...roles.map((candidate) => ({ value: candidate.id, label: candidate.name })),
              ]}
            />
          </FormField>
          <FormRow>
            {narrowable && (
              <FormField
                label={t('groups.platform.columns.scope')}
                hint={t('groups.platform.scope_hint')}
              >
                <FormSelect
                  value={organizationId}
                  disabled={controlsDisabled}
                  disabledReason={access.reason}
                  onChange={setOrganizationId}
                  options={[
                    { value: WHOLE_PLATFORM, label: t('groups.platform.whole_platform') },
                    ...tenants.map((organization) => ({
                      value: organization.id,
                      label: organization.name,
                    })),
                  ]}
                />
              </FormField>
            )}
            <FormField label={t('groups.fields.expires_at')}>
              <DateTimePicker
                value={expiresAt}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                onChange={setExpiresAt}
              />
            </FormField>
          </FormRow>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

/** An account found by what is typed: the backend searches, so the list is not filtered again. */
function AccountCombobox({
  value,
  onChange,
  disabled,
}: {
  value: grantsApi.PlatformAccount | null;
  onChange: (account: grantsApi.PlatformAccount) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation('iam');
  const [open, setOpen] = React.useState(false);
  const [typed, setTyped] = React.useState('');
  const needle = typed.trim();

  const results = useQuery({
    queryKey: ['iam', 'platform-accounts', 'search', needle],
    queryFn: () => grantsApi.searchPlatformAccounts(needle),
    enabled: open && needle !== '',
  });

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) setTyped('');
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          role="combobox"
          aria-label={t('groups.platform.columns.account')}
          aria-expanded={open}
          disabled={disabled}
          className="flex h-9 w-full min-w-0 items-center justify-between gap-2 rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-sm text-tx-1 outline-none transition-colors hover:bg-bg-3 focus-visible:bg-bg-3 disabled:opacity-50 data-[state=open]:bg-bg-3"
        >
          <span className={cn('truncate text-left', !value && 'text-tx-3')}>
            {value ? `${value.displayName} · ${value.email}` : t('groups.platform.account_placeholder')}
          </span>
          <ChevronDown aria-hidden className="h-3.5 w-3.5 shrink-0 opacity-50" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[var(--radix-popover-trigger-width)] min-w-[280px] p-0">
        <Command shouldFilter={false}>
          <CommandInput
            value={typed}
            onValueChange={setTyped}
            placeholder={t('groups.platform.account_search')}
          />
          <CommandList className="max-h-72">
            {needle !== '' && !results.isFetching && (
              <CommandEmpty>{t('groups.platform.account_none')}</CommandEmpty>
            )}
            <CommandGroup>
              {(results.data ?? []).map((account) => (
                <CommandItem
                  key={account.id}
                  value={account.id}
                  disabled={account.disabled}
                  onSelect={() => {
                    onChange(account);
                    setOpen(false);
                  }}
                  className="min-h-9"
                >
                  <span className="min-w-0 flex-1 truncate">
                    {account.displayName}
                    <span className="ml-2 text-xs text-tx-3">{account.email}</span>
                  </span>
                  {value?.id === account.id && <Check className="h-3.5 w-3.5 shrink-0" aria-hidden />}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
