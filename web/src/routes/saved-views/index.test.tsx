import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { SavedViewsRoute } from './index';

vi.mock('../SavedViews', () => ({
  SavedViews: () => <p>the list a person manages</p>,
}));

vi.mock('./report/SavedViewReport', () => ({
  SavedViewReport: () => <p>one view on a document</p>,
}));

function renderRoute(url: string) {
  return render(
    <MemoryRouter initialEntries={[url]}>
      <Routes>
        <Route path="/saved-views" element={<SavedViewsRoute />} />
      </Routes>
    </MemoryRouter>,
  );
}

afterEach(cleanup);

describe('SavedViewsRoute', () => {
  it('is the list a person manages', () => {
    renderRoute('/saved-views');

    expect(screen.getByText('the list a person manages')).toBeTruthy();
  });

  it('stays the list when a link names a view without asking for a report', () => {
    renderRoute('/saved-views?view=v1');

    expect(screen.getByText('the list a person manages')).toBeTruthy();
  });

  it('is the view alone when the report renderer asks for it', () => {
    renderRoute('/saved-views?view=v1&report_render=1&time=now-24h..now');

    expect(screen.getByText('one view on a document')).toBeTruthy();
    expect(screen.queryByText('the list a person manages')).toBeNull();
  });
});
