import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as queryApi from '@/api/query';
import i18n from '@/i18n';
import type { QueryResult } from '@/types/query';

import { QueryRecommendations } from './Recommendations';

vi.mock('@/api/query', () => ({
  recommendations: vi.fn(),
}));

const RESULT: QueryResult = {
  columns: ['message'],
  rows: [['first'], ['second']],
  scanned_rows: 24,
  took_ms: 18,
};

beforeEach(async () => {
  vi.mocked(queryApi.recommendations).mockReset();
  await i18n.changeLanguage('en-us');
});

afterEach(() => {
  cleanup();
});

function renderRecommendation() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <QueryRecommendations
        result={RESULT}
        statement="SELECT * FROM app_logs"
        language="sql"
        timeRangeSecs={43_200}
        variant="inline"
      />
    </QueryClientProvider>,
  );
}

describe('QueryRecommendations', () => {
  it('localizes the recommendation title and detail from its stable code', async () => {
    await i18n.changeLanguage('zh-cn');
    vi.mocked(queryApi.recommendations).mockResolvedValue({
      recommendations: [
        {
          code: 'select_star',
          severity: 'info',
          title: 'Avoid SELECT *',
          detail: 'Projecting all columns reads more data than needed.',
        },
      ],
    });

    renderRecommendation();
    fireEvent.click(await screen.findByRole('button', { name: '查看建议' }));

    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('避免使用 SELECT *')).not.toBeNull();
    expect(
      within(dialog).getByText(/读取全部列会产生不必要的数据扫描。请只选择实际需要的列。/),
    ).not.toBeNull();
    expect(within(dialog).queryByText('Avoid SELECT *')).toBeNull();
  });

  it('falls back to backend copy for an unknown recommendation code', async () => {
    vi.mocked(queryApi.recommendations).mockResolvedValue({
      recommendations: [
        {
          code: 'future_rule',
          severity: 'warning',
          title: 'Future rule',
          detail: 'This rule is newer than the current client.',
        },
      ],
    });

    renderRecommendation();
    fireEvent.click(await screen.findByRole('button', { name: 'View tips' }));

    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Future rule')).not.toBeNull();
    expect(
      within(dialog).getByText(/This rule is newer than the current client/),
    ).not.toBeNull();
  });
});
