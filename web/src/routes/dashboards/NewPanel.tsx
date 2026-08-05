import { useParams } from 'react-router-dom';

import { DashboardEditor } from './Editor';

/**
 * Dedicated `/dashboards/:id/panels/new` route. Mounts the existing
 * DashboardEditor; the editor reads the dashboard id from the URL and
 * focuses the new-panel surface. Save reuses the existing PATCH path,
 * which navigates back to `/dashboards/:id`.
 */
export function DashboardNewPanel() {
  useParams<{ id: string }>();
  return <DashboardEditor />;
}
