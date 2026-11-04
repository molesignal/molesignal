import { useQuery } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as routingApi from '@/api/alertRouting';
import type { RoutingRule } from '@/api/alertRouting';
import * as escalationsApi from '@/api/escalations';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { ROUTING_RULES } from '@/shell/transfer/kinds';
import { TransferMenu } from '@/shell/transfer/TransferMenu';

import { AlertsPage } from '../Frame';
import { ROUTING_RULES_KEY, ROUTING_TEAMS_KEY } from './model';
import { RoutingRuleDrawer } from './RuleDrawer';
import { RoutingRules } from './Rules';
import { RoutingTeams } from './Teams';
import { TryRouting } from './Try';

/**
 * Where an incident goes when its source leaves that to routing: the rules tried in order, the
 * on-call policy of each team, and a place to try a set of labels against both.
 */
export function AlertsRouting() {
  const { t } = useTranslation('alerts');
  const manageAccess = useActionAccess({ permission: 'alerts.routing.manage' });
  const [drawer, setDrawer] = React.useState<{ open: boolean; editing: RoutingRule | null }>({
    open: false,
    editing: null,
  });

  const rulesQuery = useQuery({ queryKey: ROUTING_RULES_KEY, queryFn: () => routingApi.listRules() });
  const teamsQuery = useQuery({ queryKey: ROUTING_TEAMS_KEY, queryFn: () => routingApi.listTeams() });
  const policiesQuery = useQuery({ queryKey: ['escalation-policies'], queryFn: () => escalationsApi.list() });

  const rules = rulesQuery.data ?? [];
  const teams = React.useMemo(() => teamsQuery.data ?? [], [teamsQuery.data]);
  const teamChoices = React.useMemo(() => teams.map((team) => ({ id: team.teamId, name: team.name })), [teams]);
  const policies = React.useMemo(
    () => (policiesQuery.data ?? []).map((policy) => ({ id: policy.id, name: policy.name })),
    [policiesQuery.data],
  );

  const queries = [rulesQuery, teamsQuery, policiesQuery];
  const failed = queries.find((query) => query.isError);
  const state: ProductStateProps | null = queries.some((query) => query.isLoading)
    ? { variant: 'loading' }
    : failed
      ? {
          variant: 'error',
          error: failed.error,
          action: (
            <ChromeButton onClick={() => queries.forEach((query) => void query.refetch())}>
              {t('center.retry')}
            </ChromeButton>
          ),
        }
      : null;

  return (
    <>
      <AlertsPage
        action={
          <>
            <TransferMenu kind={ROUTING_RULES} />
            <ChromeButton
              variant="primary"
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => setDrawer({ open: true, editing: null })}
            >
              <Plus className="h-4 w-4" />
              {t('routing.new_rule')}
            </ChromeButton>
          </>
        }
        state={state}
        contentSurface={false}
      >
        <div className="flex min-w-0 flex-col gap-[12px]">
          <RoutingRules
            rules={rules}
            policies={policies}
            teams={teamChoices}
            onEdit={(rule) => setDrawer({ open: true, editing: rule })}
          />
          <RoutingTeams teams={teams} policies={policies} />
          <TryRouting rules={rules} policies={policies} teams={teamChoices} />
        </div>
      </AlertsPage>
      <RoutingRuleDrawer
        open={drawer.open}
        editing={drawer.editing}
        policies={policies}
        teams={teamChoices}
        onClose={() => setDrawer((current) => ({ ...current, open: false }))}
      />
    </>
  );
}
