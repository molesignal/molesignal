import * as alertsApi from '@/api/alerts';
import { TransferMenu, type TransferKind } from '@/shell/transfer/TransferMenu';

const RULES: TransferKind = {
  testPrefix: 'rule',
  texts: 'rule_transfer',
  fileName: 'molesignal-alert-rules',
  readPermission: 'alerts.rules.read',
  managePermission: 'alerts.rules.manage',
  exportBundle: async (ids) => {
    const bundle = await alertsApi.exportRules(ids);
    return { bundle, count: bundle.rules.length };
  },
  importBundle: alertsApi.importRules,
  queryKey: ['alerts', 'rules'],
  placeholder:
    'groups:\n  - name: api\n    rules:\n      - alert: HighErrorRate\n        expr: sum(rate(http_requests_total{code=~"5.."}[5m])) > 1\n        for: 5m',
};

/**
 * Rules as a file: exported as a bundle that names policies and child rules, and imported from
 * such a bundle or from a Prometheus rule file, tried first and written at once.
 */
export function RuleTransferMenu({ ruleIds }: { ruleIds: string[] }) {
  return <TransferMenu kind={RULES} ids={ruleIds} />;
}
