import '@testing-library/jest-dom/vitest';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as statusPagesApi from '@/api/statusPages';
import type {
  AutomationCandidate,
  AutomationCandidateDetail,
  StatusPageComponent,
  StatusPageIncident,
} from '@/api/statusPages';
import i18n from '@/i18n';

import { CandidateReviewDrawer } from './CandidateReviewDrawer';

vi.mock('@/api/statusPages', () => ({
  approveAutomationCandidate: vi.fn(),
  getAutomationCandidateDetail: vi.fn(),
  getEvent: vi.fn(),
  rejectAutomationCandidate: vi.fn(),
  retryAutomationCandidate: vi.fn(),
}));

const getCandidate = vi.mocked(statusPagesApi.getAutomationCandidateDetail);
const getEvent = vi.mocked(statusPagesApi.getEvent);

const components: StatusPageComponent[] = [
  {
    id: 'component-api',
    orgId: 'org-one',
    statusPageId: 'page-one',
    name: 'Web API',
    description: '',
    status: 'operational',
    visibility: 'enabled',
    lifecycle: 'active',
    position: 0,
    archivedAt: null,
    createdAt: 1,
    updatedAt: 1,
  },
];

function candidate(statusIncidentId: string | null): AutomationCandidate {
  return {
    id: 'candidate-one',
    organizationId: 'org-one',
    statusPageId: 'page-one',
    ruleRevisionId: 'revision-one',
    correlationKey: 'api-latency',
    state: 'pending_approval',
    title: 'API latency degradation',
    message: 'We are investigating elevated API latency.',
    resolvedMessage: 'API latency has recovered.',
    impact: 'major',
    componentIds: ['component-api'],
    automatic: false,
    statusIncidentId: statusIncidentId,
    dueAt: 2,
    createdAt: 1,
    updatedAt: 2,
    lastError: null,
  };
}

function detail(value: AutomationCandidate): AutomationCandidateDetail {
  return { candidate: value, sources: [], actions: [], workItems: [] };
}

function incident(): StatusPageIncident {
  return {
    id: 'incident-one',
    orgId: 'org-one',
    statusPageId: 'page-one',
    sourceIncidentId: null,
    kind: 'incident',
    title: 'API latency degradation',
    impact: 'major',
    status: 'investigating',
    publicationState: 'draft',
    draftMessage: 'We are investigating elevated API latency.',
    componentIds: ['component-api'],
    updates: [],
    startedAt: 1,
    endedAt: null,
    publishedAt: null,
    createdAt: 1,
    updatedAt: 2,
  };
}

function renderDrawer() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <CandidateReviewDrawer
        pageId="page-one"
        candidateId="candidate-one"
        components={components}
        canPublish
        onClose={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

describe('CandidateReviewDrawer', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    getCandidate.mockReset();
    getEvent.mockReset();
  });

  afterEach(() => cleanup());

  it('explains why approval is disabled when the candidate has no linked draft', async () => {
    getCandidate.mockResolvedValue(detail(candidate(null)));

    renderDrawer();

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This candidate has no linked draft incident.',
    );
    expect(screen.getByRole('button', { name: 'Approve and publish' })).toBeDisabled();
    expect(getEvent).not.toHaveBeenCalled();
  });

  it('enables approval after the linked draft loads', async () => {
    getCandidate.mockResolvedValue(detail(candidate('incident-one')));
    getEvent.mockResolvedValue(incident());

    renderDrawer();

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Approve and publish' })).toBeEnabled();
    });
    expect(screen.getByDisplayValue('API latency degradation')).toBeInTheDocument();
  });
});
