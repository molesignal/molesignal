import { LayoutDashboard } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumApplication } from '@/api/rum';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { useAuthStore } from '@/stores/auth';

/**
 * Where the built-in dashboard of an application's platform is, with the application and the
 * environment already chosen in it; none for a platform the dashboards have nothing for. The
 * dashboard is the organization's own, whose id is made of its uid and the organization.
 */
export function rumDashboardPath(
  platform: RumApplication['platform'],
  orgId: string,
  application: string,
  environment: string,
): string | null {
  if (platform === 'other' || !orgId) return null;

  const uid = platform === 'browser' ? 'rum-browser' : 'rum-mobile';
  const params = new URLSearchParams({ 'var-application': application });

  if (environment) params.set('var-environment', environment);

  return `/dashboards/${uid}-${orgId}?${params}`;
}

/**
 * The dashboard of the application, in the head of the overview. Those who may not read dashboards
 * do not see it.
 */
export function RumDashboardLink({
  application,
  environment,
  className,
}: {
  application: RumApplication;
  environment: string;
  /** The look of an action of the head. */
  className?: string | undefined;
}) {
  const { t } = useTranslation('rum');
  const reader = useActionAccess({ permission: 'dashboards.read' });
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const path = rumDashboardPath(application.platform, orgId, application.id, environment);

  if (!reader.allowed || !path) return null;

  return (
    <Link
      to={path}
      data-rum-dashboard-link=""
      className={cn(
        'inline-flex shrink-0 items-center gap-1.5 rounded-md font-sans font-strong text-tx-1 outline-none transition-colors',
        className,
      )}
    >
      <LayoutDashboard aria-hidden className="h-3.5 w-3.5 text-tx-3" />
      {t('overview.open_dashboard')}
    </Link>
  );
}
