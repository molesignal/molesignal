import { Outlet } from 'react-router-dom';

/** The APM module: each page draws its own navigation card, the module's or a service's. */
export function ApmLayout() {
  return <Outlet />;
}
