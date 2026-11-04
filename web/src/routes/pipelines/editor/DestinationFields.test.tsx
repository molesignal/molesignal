import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { Connector } from '@/api/connectors';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { DestinationFields } from './DestinationFields';
import type { PipelineResources } from './resources';
import { defaultPipelineGraph, type PipelineGraphModel } from '../PipelineGraph';

const MODEL: PipelineGraphModel = {
  ...defaultPipelineGraph('logs'),
  sources: ['app_logs'],
  sinks: ['clean_logs', 'connector:c1'],
};

function archive(enabled: boolean): Connector {
  return {
    id: 'c1',
    name: 'Archive',
    kind: 's3',
    configJson: { bucket: 'acme-logs' },
    enabled,
    createdAt: 0,
    updatedAt: 0,
  };
}

function resources(connectors: Connector[]): PipelineResources {
  return {
    streams: [],
    streamsPending: false,
    streamsFailed: false,
    connectors,
    connectorIds: connectors.map((connector) => connector.id),
    connectorNames: Object.fromEntries(connectors.map((connector) => [connector.id, connector.name])),
    tables: [],
  };
}

function renderConnectorDestination(connectors: Connector[]) {
  render(
    <TooltipProvider delayDuration={0}>
      <DestinationFields model={MODEL} index={1} resources={resources(connectors)} onChange={vi.fn()} />
    </TooltipProvider>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

describe('DestinationFields', () => {
  it('says every event goes to a connector that is on', () => {
    renderConnectorDestination([archive(true)]);

    expect(screen.getByText('Archive (s3)')).not.toBeNull();
    expect(screen.getByText('Every event the pipeline writes is also delivered to this connector.')).not.toBeNull();
  });

  it('says runs skip a connector that is off', () => {
    renderConnectorDestination([archive(false)]);

    expect(screen.getByText('Archive (s3) · Off')).not.toBeNull();
    expect(screen.getByText('This connector is off: runs deliver nothing to it until it is turned on.')).not.toBeNull();
    expect(screen.queryByText('Every event the pipeline writes is also delivered to this connector.')).toBeNull();
  });
});
