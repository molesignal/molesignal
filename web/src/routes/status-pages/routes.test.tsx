import { createMemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import { STATUS_PAGE_MANAGEMENT_ROUTES } from './routes';

describe('status page management routes', () => {
  it.each([
    ['components', 'componentId'],
    ['incidents', 'eventId'],
    ['maintenance', 'eventId'],
  ])('exposes new as the %s drawer route parameter', (section, parameter) => {
    const router = createMemoryRouter(STATUS_PAGE_MANAGEMENT_ROUTES, {
      initialEntries: [`/status-pages/page-1/${section}/new`],
    });

    expect(router.state.matches.at(-1)?.params).toMatchObject({
      pageId: 'page-1',
      [parameter]: 'new',
    });
    router.dispose();
  });
});
