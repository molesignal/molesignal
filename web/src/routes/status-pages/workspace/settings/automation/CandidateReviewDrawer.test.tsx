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
    org_id: 'org-one',
    status_page_id: 'page-one',
    name: 'Web API',
    description: '',
    status: 'operational',
    visibility: 'enabled',
    lifecycle: 'active',
    position: 0,
    archived_at: null,
    created_at: 1,
    updated_at: 1,
  },
];

function candidate(statusIncidentId: string | null): AutomationCandidate {
  return {
    id: 'candidate-one',
    organization_id: 'org-one',
    status_page_id: 'page-one',
    rule_revision_id: 'revision-one',
    correlation_key: 'api-latency',
    state: 'pending_approval',
    title: 'API latency degradation',
    message: 'We are investigating elevated API latency.',
    resolved_message: 'API latency has recovered.',
    impact: 'major',
    component_ids: ['component-api'],
    automatic: false,
    status_incident_id: statusIncidentId,
    due_at: 2,
    created_at: 1,
    updated_at: 2,
    last_error: null,
  };
}

function detail(value: AutomationCandidate): AutomationCandidateDetail {
  return { candidate: value, sources: [], actions: [], work_items: [] };
}

function incident(): StatusPageIncident {
  return {
    id: 'incident-one',
    org_id: 'org-one',
    status_page_id: 'page-one',
    source_incident_id: null,
    kind: 'incident',
    title: 'API latency degradation',
    impact: 'major',
    status: 'investigating',
    publication_state: 'draft',
    draft_message: 'We are investigating elevated API latency.',
    component_ids: ['component-api'],
    updates: [],
    started_at: 1,
    ended_at: null,
    published_at: null,
    created_at: 1,
    updated_at: 2,
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
