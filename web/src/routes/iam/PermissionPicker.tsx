import { Check, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import {
  type PermissionCatalog,
  type PermissionDefinition,
  type PermissionKey,
  type PermissionScope,
  permissionDefinitionsByDomain,
} from '@/product/permissions';
import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';

export interface PermissionGroup {
  domain: string;
  permissions: PermissionDefinition[];
}

/** The catalog's permissions of a scope among `keys`, by domain in catalog order. */
export function permissionGroupsFor(
  catalog: PermissionCatalog | undefined,
  keys: readonly string[],
  scope: PermissionScope = 'organization',
): PermissionGroup[] {
  const wanted = new Set(keys);

  return permissionDefinitionsByDomain(catalog, scope)
    .map((group) => ({
      domain: group.domain,
      permissions: group.permissions.filter((permission) => wanted.has(permission.key)),
    }))
    .filter((group) => group.permissions.length > 0);
}

/** Permissions to read, not to change: what a policy allows, one line each. */
export function PermissionList({ groups }: { groups: PermissionGroup[] }) {
  const { t } = useTranslation('iam');

  return (
    <div className="flex flex-col gap-3" data-permission-list>
      {groups.map((group) => (
        <section key={group.domain}>
          <h4 className="px-2 pb-1 font-sans text-xs font-medium text-tx-2">
            {t(`permission_domains.${group.domain}`)}
          </h4>
          <ul className="grid grid-cols-[auto_max-content_minmax(0,1fr)] gap-x-2.5">
            {group.permissions.map((permission) => {
              const hint = t(permission.descriptionKey);

              return (
                <li
                  key={permission.key}
                  title={hint}
                  className="col-span-3 grid h-8 grid-cols-subgrid items-center px-2 font-sans"
                >
                  <Check className="h-3.5 w-3.5 text-indigo" aria-hidden />
                  <span className="text-sm text-tx-0">{t(permission.labelKey)}</span>
                  <span className="truncate text-xs text-tx-3">{hint}</span>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}

/**
 * Ticks permissions from the catalog, one line each: the name, then what it
 * allows. A search narrows the list by name, description or key; what is
 * ticked stays ticked while it is hidden. A permission granted another way, by
 * a policy a role attaches, shows ticked with where it comes from, and cannot
 * be unticked here.
 */
export function PermissionPicker({
  groups,
  selected,
  onChange,
  inherited,
  disabled = false,
  disabledReason,
}: {
  groups: PermissionGroup[];
  selected: readonly PermissionKey[];
  onChange: (next: PermissionKey[]) => void;
  /** Permissions granted another way, each with the names of what grants it. */
  inherited?: ReadonlyMap<string, string>;
  disabled?: boolean;
  disabledReason?: React.ReactNode;
}) {
  const { t } = useTranslation('iam');
  const [query, setQuery] = React.useState('');
  const needle = query.trim().toLowerCase();
  const ticked = new Set<string>(selected);
  const granted = (key: string) => ticked.has(key) || Boolean(inherited?.has(key));

  const shown = groups
    .map((group) => ({
      domain: group.domain,
      total: group.permissions.length,
      ticked: group.permissions.filter((permission) => granted(permission.key)).length,
      permissions: group.permissions
        .map((permission) => ({
          key: permission.key,
          label: t(permission.labelKey),
          hint: t(permission.descriptionKey),
          from: ticked.has(permission.key) ? undefined : inherited?.get(permission.key),
        }))
        .filter(
          (permission) =>
            !needle ||
            [permission.label, permission.hint, permission.key].some((text) =>
              text.toLowerCase().includes(needle),
            ),
        ),
    }))
    .filter((group) => group.permissions.length > 0);

  const toggle = (key: PermissionKey, checked: boolean) => {
    if (disabled) return;
    onChange(checked ? [...selected, key] : selected.filter((value) => value !== key));
  };

  return (
    <div className="flex flex-col gap-3" data-permission-picker>
      <label className="flex h-9 min-w-0 items-center gap-2 rounded-md bg-[var(--control-surface)] px-3">
        <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" aria-hidden />
        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={t('permission_picker.search_placeholder')}
          aria-label={t('permission_picker.search_aria')}
          className="min-w-0 flex-1 bg-transparent font-sans text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none"
        />
      </label>
      <DisabledControl disabled={disabled} reason={disabledReason} className="w-full">
        {shown.length === 0 ? (
          <p className="py-6 text-center font-sans text-xs text-tx-3">
            {t('permission_picker.no_match')}
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            {shown.map((group) => (
              <section
                key={group.domain}
                className="grid grid-cols-[auto_max-content_minmax(0,1fr)] gap-x-2.5"
              >
                <h4 className="col-span-3 flex items-baseline justify-between px-2 pb-1 font-sans text-xs font-medium text-tx-2">
                  {t(`permission_domains.${group.domain}`)}
                  <span className="font-normal tabular-nums text-tx-3">
                    {group.ticked}/{group.total}
                  </span>
                </h4>
                {group.permissions.map((permission) => {
                  const locked = disabled || permission.from !== undefined;
                  const from = permission.from
                    ? t('permission_picker.inherited', { from: permission.from })
                    : undefined;

                  return (
                    <label
                      key={permission.key}
                      title={from ?? permission.hint}
                      data-permission-inherited={permission.from !== undefined || undefined}
                      className={cn(
                        'col-span-3 grid h-8 grid-cols-subgrid items-center rounded-md px-2 font-sans',
                        locked ? 'cursor-not-allowed' : 'cursor-pointer hover:bg-bg-2',
                      )}
                    >
                      <input
                        type="checkbox"
                        checked={granted(permission.key)}
                        disabled={locked}
                        aria-label={permission.label}
                        onChange={(event) => toggle(permission.key, event.currentTarget.checked)}
                        className="h-3.5 w-3.5 cursor-pointer accent-indigo disabled:cursor-not-allowed"
                      />
                      <span className={cn('text-sm', disabled ? 'text-tx-3' : 'text-tx-0')}>
                        {permission.label}
                      </span>
                      <span className="truncate text-xs text-tx-3">{from ?? permission.hint}</span>
                    </label>
                  );
                })}
              </section>
            ))}
          </div>
        )}
      </DisabledControl>
    </div>
  );
}
