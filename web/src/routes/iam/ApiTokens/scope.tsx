import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as apiTokens from '@/api/apiTokens';
import type { ApiToken, ApiTokenKind, CreateApiTokenPayload, TokenScopes } from '@/api/apiTokens';
import { useProductAccess } from '@/product/access';
import {
  type PermissionCatalog,
  type PermissionKey,
  permissionDefinitionsByDomain,
} from '@/product/permissions';
import { FormField, FormRadio, FormSection, FormSelect } from '@/shell/FormDrawer';

import { PermissionList, PermissionPicker, permissionGroupsFor } from '../PermissionPicker';
import { policyDescription, policyName } from '../Policies/labels';

/** How a new token's scope is chosen. */
export type ScopeChoice =
  | { kind: 'principal' }
  | { kind: 'policy'; policyId: string }
  | { kind: 'permissions'; permissions: PermissionKey[] };

/** Whose access a token narrows: its issuer's own, or a service account's. */
export type TokenPrincipal = 'me' | 'service_account';

export const PRINCIPAL_SCOPE: ScopeChoice = { kind: 'principal' };

export const TOKEN_SCOPES_QUERY_KEY = ['me', 'api-tokens', 'scopes'] as const;

/** The policies and permissions a token's scope is named with and picked from. */
export function useTokenScopes() {
  return useQuery({
    queryKey: TOKEN_SCOPES_QUERY_KEY,
    queryFn: apiTokens.scopes,
    staleTime: 60_000,
    retry: false,
  });
}

/** The scopes' permissions in the shape the permission lists read. */
function catalogOf(scopes: TokenScopes | undefined): PermissionCatalog | undefined {
  return scopes ? { version: 0, permissions: scopes.permissions } : undefined;
}

/** The request fields for a scope choice; neither field leaves the principal's access whole. */
export function scopePayload(
  choice: ScopeChoice,
): Pick<CreateApiTokenPayload, 'policyId' | 'permissions'> {
  if (choice.kind === 'policy') return { policyId: choice.policyId };
  if (choice.kind === 'permissions') return { permissions: choice.permissions };
  return {};
}

/** A choice the server would refuse: a policy not picked, or no permission ticked. */
export function scopeIncomplete(choice: ScopeChoice): boolean {
  if (choice.kind === 'policy') return choice.policyId === '';
  if (choice.kind === 'permissions') return choice.permissions.length === 0;
  return false;
}

function principalOf(kind: ApiTokenKind): TokenPrincipal {
  return kind === 'service_account' ? 'service_account' : 'me';
}

type ScopedToken = Pick<ApiToken, 'policyId' | 'permissions' | 'tokenKind'>;

/** What a listed token may do: in a few words, the permissions behind them, and whether it is simply all its principal can. */
function describeScope(
  token: ScopedToken,
  scopes: TokenScopes | undefined,
  t: TFunction<'iam'>,
): { words: string; detail?: string; principal?: boolean } {
  const labelsOf = (keys: readonly string[]) =>
    keys.map((key) => {
      const definition = scopes?.permissions.find((permission) => permission.key === key);
      return definition ? t(definition.labelKey) : key;
    });

  if (token.policyId) {
    const policy = scopes?.policies.find((candidate) => candidate.id === token.policyId);

    return policy
      ? { words: policyName(policy, t), detail: labelsOf(policy.permissions).join('、') }
      : { words: token.policyId };
  }

  const permissions = token.permissions ?? [];

  if (permissions.length === 0) {
    return { words: t(`api_tokens.scope.principal_label.${principalOf(token.tokenKind)}`), principal: true };
  }

  const labels = labelsOf(permissions);

  return labels.length === 1
    ? { words: labels[0] ?? '' }
    : { words: t('api_tokens.scope.custom_label', { count: labels.length }), detail: labels.join('、') };
}

/** The same words as `TokenScopeLabel`, for a file. */
export function useTokenScopeWords(): (token: ScopedToken) => string {
  const { t } = useTranslation('iam');
  const scopes = useTokenScopes().data;

  return (token) => describeScope(token, scopes, t).words;
}

/** What a listed token may do, in a few words. */
export function TokenScopeLabel({ token }: { token: ScopedToken }) {
  const { t } = useTranslation('iam');
  const { words, detail, principal } = describeScope(token, useTokenScopes().data, t);

  return (
    <span title={detail} className={principal ? 'text-tx-2' : undefined}>
      {words}
    </span>
  );
}

/** Picks what a new token may do, from what the issuer holds. */
export function TokenScopeField({
  value,
  onChange,
  principal,
  disabled = false,
}: {
  value: ScopeChoice;
  onChange: (next: ScopeChoice) => void;
  principal: TokenPrincipal;
  disabled?: boolean;
}) {
  const { t } = useTranslation('iam');
  const scopes = useTokenScopes().data;
  const catalog = catalogOf(scopes);
  const access = useProductAccess();
  const held = React.useMemo(() => access?.permissions ?? new Set<PermissionKey>(), [access]);

  const policies = (scopes?.policies ?? []).map((policy) => {
    const missing = policy.permissions.some((permission) => !held.has(permission as PermissionKey));

    return {
      value: policy.id,
      label: policyName(policy, t),
      disabled: missing,
      ...(missing ? { disabledReason: t('api_tokens.scope.policy_not_held') } : {}),
    };
  });

  const groups = permissionDefinitionsByDomain(catalog)
    .map((group) => ({
      ...group,
      permissions: group.permissions.filter((permission) => held.has(permission.key)),
    }))
    .filter((group) => group.permissions.length > 0);

  const chosenPolicy =
    value.kind === 'policy'
      ? scopes?.policies.find((policy) => policy.id === value.policyId)
      : undefined;
  const chosenDescription = chosenPolicy ? policyDescription(chosenPolicy, t) : '';

  return (
    <FormSection title={t('api_tokens.fields.scope')}>
      <FormRadio<ScopeChoice['kind']>
        value={value.kind}
        onChange={(kind) => {
          if (disabled) return;
          if (kind === 'principal') onChange(PRINCIPAL_SCOPE);
          if (kind === 'policy') onChange({ kind, policyId: '' });
          if (kind === 'permissions') onChange({ kind, permissions: [] });
        }}
        options={[
          {
            value: 'principal',
            label: t(`api_tokens.scope.principal.${principal}`),
            hint: t(`api_tokens.scope.principal_hint.${principal}`),
          },
          {
            value: 'policy',
            label: t('api_tokens.scope.policy'),
            hint: t('api_tokens.scope.policy_hint'),
          },
          {
            value: 'permissions',
            label: t('api_tokens.scope.custom'),
            hint: t('api_tokens.scope.custom_hint'),
          },
        ]}
      />
      {value.kind === 'policy' && (
        <FormField
          label={t('api_tokens.fields.policy')}
          required
          {...(chosenDescription ? { hint: chosenDescription } : {})}
        >
          <FormSelect
            value={value.policyId}
            onChange={(policyId) => onChange({ kind: 'policy', policyId })}
            disabled={disabled}
            options={[
              { value: '', label: t('api_tokens.fields.policy_placeholder') },
              ...policies,
            ]}
          />
        </FormField>
      )}
      {chosenPolicy && (
        <PermissionList groups={permissionGroupsFor(catalog, chosenPolicy.permissions)} />
      )}
      {value.kind === 'permissions' && (
        <PermissionPicker
          groups={groups}
          selected={value.permissions}
          onChange={(permissions) => onChange({ kind: 'permissions', permissions })}
          disabled={disabled}
        />
      )}
    </FormSection>
  );
}
