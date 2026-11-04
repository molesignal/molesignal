import { Bot } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumAggregateParams } from '@/api/rum';

import { formatCount } from './format';
import { explorerPath, type OverviewRange, scopeFilters } from './model';
import { useAggregate } from './useOverview';

/** The kinds of session a test drives. */
const TEST_KINDS = ['synthetics', 'ci_test'];

/** The read that tells it: the sessions of the application that no person ran. */
export function testSessionsQuery(application: string, environment: string, range: OverviewRange): RumAggregateParams {
  return {
    type: 'sessions',
    from: range.from,
    to: range.to,
    filters: scopeFilters({ application, environment }, [{ field: 'session.type', op: 'in', value: JSON.stringify(TEST_KINDS) }]),
    measures: [{ fn: 'stored', name: 'sessions' }],
    estimate: false,
  };
}

/**
 * Says that a page leaves out the sessions tests drove, when there are any: the figures of an application
 * are of real users. It leads to those sessions.
 */
export function TestTrafficNote({
  application,
  environment,
  range,
  language,
}: {
  application: string;
  environment: string;
  range: OverviewRange;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const read = useAggregate('test-sessions', testSessionsQuery(application, environment, range));
  const value = read.data?.rows[0]?.values[0];
  const sessions = typeof value === 'number' ? value : 0;

  if (sessions <= 0) return null;

  return (
    <Link
      data-rum-test-traffic={sessions}
      to={explorerPath('sessions', { application, environment }, [`session.type != 'user'`, `session.type != ''`])}
      title={t('overview.test_traffic.title')}
      className="inline-flex shrink-0 items-center gap-1 font-sans text-xs text-tx-3 outline-none hover:text-tx-1 focus-visible:text-tx-1"
    >
      <Bot aria-hidden className="h-3.5 w-3.5" />
      {t('overview.test_traffic.note', { count: sessions, value: formatCount(sessions, language) })}
    </Link>
  );
}
