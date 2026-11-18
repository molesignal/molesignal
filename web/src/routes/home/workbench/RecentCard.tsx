import {
  Activity,
  Bell,
  Bookmark,
  LayoutDashboard,
  type LucideIcon,
  Radar,
  RadioTower,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { isProductPathDenied, useProductAccess } from '@/product/access';
import { useRecentVisits, type VisitKind } from '@/shell/recentVisits';
import { formatRelativeMicros } from '@/time/relative';

import { CanvasSection } from '../canvas/layout';

/** The places shown: two rows of three. */
const SHOWN_VISITS = 6;

const KIND_ICON: Record<VisitKind, LucideIcon> = {
  dashboard: LayoutDashboard,
  service: Activity,
  incident: Bell,
  saved_view: Bookmark,
  check: Radar,
  status_page: RadioTower,
};

/** The places the reader opened lately, on any device, the latest first. */
export function RecentCard({ className }: { className?: string }) {
  const { t, i18n } = useTranslation('onboarding');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const access = useProductAccess();
  const recent = useRecentVisits();
  // A place the reader can no longer open is left out, not offered and then refused.
  const visits = (recent.value?.visits ?? [])
    .filter((visit) => !isProductPathDenied(visit.path.split(/[?#]/, 1)[0] ?? visit.path, access))
    .slice(0, SHOWN_VISITS);
  const nowMicros = Date.now() * 1000;

  return (
    <CanvasSection
      title={t('workbench.recent.title')}
      className={className}
      bodyClassName="px-3 pb-3"
      testId="workbench-recent"
    >
      {visits.length === 0 ? (
        <p className="px-2 pb-2 pt-1 font-sans text-sm text-tx-3">
          {recent.settled ? t('workbench.recent.empty') : t('workbench.loading')}
        </p>
      ) : (
        <ul className="wb-recent-grid">
          {visits.map((visit) => {
            const Icon = KIND_ICON[visit.kind];
            return (
              <li key={`${visit.kind}:${visit.id}`} className="min-w-0">
                <Link
                  to={visit.path}
                  className="group flex min-w-0 items-center gap-3 rounded-md px-2 py-2 transition-colors duration-fast hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none"
                >
                  <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-bg-2 text-tx-2 transition-colors duration-fast group-hover:bg-bg-3 group-hover:text-tx-1">
                    <Icon aria-hidden="true" className="h-4 w-4" />
                  </span>
                  <span className="min-w-0">
                    <span className="block truncate font-sans text-sm font-strong text-tx-0">{visit.label}</span>
                    <span className="mt-0.5 block truncate font-sans text-xs text-tx-3">
                      {t(`workbench.kinds.${visit.kind}`)} · {formatRelativeMicros(visit.visitedAt, locale, nowMicros)}
                    </span>
                  </span>
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </CanvasSection>
  );
}
