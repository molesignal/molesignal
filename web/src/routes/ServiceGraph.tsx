import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { PageBody, PageHeader } from '@/shell/PageHeader';
import { ServiceTopology } from '@/viz/topology/ServiceTopology';

export function ServiceGraph() {
  const { t } = useTranslation(['nav', 'shell']);
  const now = React.useMemo(() => new Date(), []);
  const from = React.useMemo(() => new Date(now.getTime() - 3600_000).toISOString(), [now]);
  const to = React.useMemo(() => now.toISOString(), [now]);

  // No `onNodeClick` — DESIGN_REVIEW.md could-improve #3. Each ServiceNode
  // service label is already wrapped in a `SignalReference(type=service)`
  // (see `viz/topology/ServiceNode.tsx`), and its HoverCard offers the same
  // "Traces for this service" jump plus Logs and Service-graph. Letting the
  // node circle also navigate to traces creates two click affordances for
  // the same outcome — Principle #2 ("one grammar"). Traces.tsx still passes
  // `onNodeClick` because there it is *panel selection*, not navigation.
  return (
    <>
      <PageHeader
        title={t('nav:service_graph')}
        subtitle={t('shell:pages.service_graph.subtitle')}
      />
      <PageBody>
        <div className="h-[70vh] min-h-[400px]">
          <ServiceTopology from={from} to={to} />
        </div>
      </PageBody>
    </>
  );
}
