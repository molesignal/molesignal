import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { IamPolicy } from '@/api/policies';
import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';

import { policyDescription, policyName } from './labels';

/** Ticks the policies a role attaches, one line each: name, what it is for, how much it allows. */
export function PolicyChecklist({
  policies,
  selected,
  onChange,
  disabled = false,
  disabledReason,
}: {
  policies: readonly IamPolicy[];
  selected: readonly string[];
  onChange: (next: string[]) => void;
  disabled?: boolean;
  disabledReason?: React.ReactNode;
}) {
  const { t } = useTranslation('iam');
  const ticked = new Set(selected);

  if (policies.length === 0) {
    return <p className="m-0 font-sans text-xs text-tx-3">{t('roles.no_policies')}</p>;
  }

  return (
    <DisabledControl disabled={disabled} reason={disabledReason} className="w-full">
      <div
        className="grid grid-cols-[auto_max-content_minmax(0,1fr)_auto] gap-x-2.5"
        data-policy-checklist
      >
        {policies.map((policy) => {
          const name = policyName(policy, t);
          const description = policyDescription(policy, t);

          return (
            <label
              key={policy.id}
              title={description}
              className={cn(
                'col-span-4 grid h-8 grid-cols-subgrid items-center rounded-md px-2 font-sans',
                disabled ? 'cursor-not-allowed' : 'cursor-pointer hover:bg-bg-2',
              )}
            >
              <input
                type="checkbox"
                checked={ticked.has(policy.id)}
                disabled={disabled}
                aria-label={name}
                onChange={(event) =>
                  onChange(
                    event.currentTarget.checked
                      ? [...selected, policy.id]
                      : selected.filter((id) => id !== policy.id),
                  )
                }
                className="h-3.5 w-3.5 cursor-pointer accent-indigo disabled:cursor-not-allowed"
              />
              <span className={cn('text-sm', disabled ? 'text-tx-3' : 'text-tx-0')}>{name}</span>
              <span className="truncate text-xs text-tx-3">{description}</span>
              <span className="text-xs tabular-nums text-tx-3">
                {t('roles.policy_permission_count', { count: policy.permissions.length })}
              </span>
            </label>
          );
        })}
      </div>
    </DisabledControl>
  );
}
