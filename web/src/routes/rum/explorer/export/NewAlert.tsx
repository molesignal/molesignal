import { BellPlus } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { QueryIconAction } from '@/shell/query/IconAction';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/shell/ui/dropdown-menu';

import { columnLabel } from '../labels';
import type { RumTab } from '../model';
import { rumAlertHref, type RumAlertPlan } from './alertSource';
import { measureLabel } from '../viz/labels';

/**
 * A new alert rule on what the explorer draws, against a threshold or against the same time on
 * the days before: the editor opens on the query and its first number. The menu says first what
 * the rule takes and what it cannot watch: the way of drawing, the groups, the other numbers.
 */
export function RumNewAlert({ plan, tab, ready }: { plan: RumAlertPlan; tab: RumTab; ready: boolean }) {
  const { t } = useTranslation('rum');
  const navigate = useNavigate();
  const access = useActionAccess({ permission: 'alerts.rules.manage' });
  const list = (items: readonly string[]) => items.join(t('explorer.export.alert.separator'));

  const left = [
    ...(plan.viz ? [t('explorer.export.alert.left_viz', { viz: t(`explorer.viz.names.${plan.viz}`) })] : []),
    ...(plan.groups.length > 0
      ? [t('explorer.export.alert.left_groups', { groups: list(plan.groups.map((group) => columnLabel(t, group))) })]
      : []),
    ...(plan.measures.length > 0
      ? [t('explorer.export.alert.left_measures', { measures: list(plan.measures.map((measure) => measureLabel(t, measure, tab))) })]
      : []),
  ];

  const watched = plan.distribution
    ? t('explorer.export.alert.watches_distribution', { field: columnLabel(t, plan.measure.field ?? '') })
    : t('explorer.export.alert.watches', { measure: measureLabel(t, plan.measure, tab) });

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <QueryIconAction
          icon={BellPlus}
          label={t('explorer.export.alert.action')}
          data-rum-new-alert=""
          disabled={access.disabled || !ready}
          disabledReason={access.disabled ? access.reason : undefined}
        />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-80" data-rum-new-alert-menu="">
        <div className="space-y-1 px-2 py-1.5 font-sans text-xs">
          <p data-rum-alert-watches="" className="m-0 text-tx-1">
            {watched}
          </p>
          {left.length > 0 && (
            <p data-rum-alert-left="" className="m-0 text-tx-3">
              {t('explorer.export.alert.left', { parts: list(left) })}
            </p>
          )}
          {!plan.application && (
            <p data-rum-alert-application="" className="m-0 text-tx-3">
              {t('explorer.export.alert.application')}
            </p>
          )}
        </div>
        <DropdownMenuSeparator />
        {(['threshold', 'anomaly'] as const).map((kind) => (
          <DropdownMenuItem
            key={kind}
            data-rum-new-alert-kind={kind}
            onSelect={() => navigate(rumAlertHref(plan, kind))}
            className="flex-col items-start gap-0.5 font-sans text-xs"
          >
            <span className="font-strong text-tx-0">{t(`explorer.export.alert.kinds.${kind}`)}</span>
            <span className="text-tx-3">{t(`explorer.export.alert.kinds.${kind}_hint`)}</span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
