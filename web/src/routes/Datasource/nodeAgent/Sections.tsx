import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as nodeAgentApi from '@/api/nodeAgent';

import { HostAssets } from './HostAssets';
import { assetRows, assetsDone, NODE_AGENT_ASSETS_KEY } from './model';
import { RUNBOOK } from './runbook';
import { GuideSection } from '../guide/Section';
import { StepBlock } from '../guide/StepBlock';

/**
 * The two steps the Node Agent guide adds after the data arrives: install what the organization
 * gets for its hosts, and what to do when something does not arrive.
 */
export function NodeAgentSections({ verified, tried }: { verified: boolean; tried: boolean }) {
  const { t } = useTranslation('onboarding');
  const query = useQuery({
    queryKey: NODE_AGENT_ASSETS_KEY,
    queryFn: nodeAgentApi.assets,
    retry: false,
  });

  const done = query.data ? assetsDone(query.data, assetRows(query.data)) : false;

  return (
    <>
      <GuideSection
        number={4}
        state={done ? 'done' : verified ? 'current' : 'upcoming'}
        title={t('datasource_page.node_agent.assets.title')}
        description={t('datasource_page.node_agent.assets.description')}
      >
        <HostAssets query={query} />
      </GuideSection>
      <GuideSection
        number={5}
        state={tried && !verified ? 'attention' : 'upcoming'}
        title={t('datasource_page.node_agent.runbook.title')}
        description={t('datasource_page.node_agent.runbook.description')}
      >
        <div data-node-agent-runbook="" className="space-y-[22px]">
          {RUNBOOK.map((entry) => (
            <StepBlock
              key={entry.id}
              text={{
                title: t(`datasource_page.node_agent.runbook.items.${entry.id}.title`),
                description: t(`datasource_page.node_agent.runbook.items.${entry.id}.description`),
              }}
              code={{ language: entry.code.lang, content: entry.code.content }}
            />
          ))}
        </div>
      </GuideSection>
    </>
  );
}
