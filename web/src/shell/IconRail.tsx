import {
  Activity,
  AlertTriangle,
  BarChart3,
  Bookmark,
  LayoutDashboard,
  type LucideIcon,
  Settings,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

interface RailItem {
  to: string;
  /** i18n key under `nav.*` */
  labelKey: string;
  icon: LucideIcon;
}

const ITEMS: RailItem[] = [
  { to: '/apm', labelKey: 'apm', icon: Activity },
  { to: '/dashboards', labelKey: 'dashboards', icon: LayoutDashboard },
  { to: '/alerts/incidents', labelKey: 'incidents', icon: AlertTriangle },
  { to: '/alerts/rules', labelKey: 'alert_rules', icon: BarChart3 },
  { to: '/saved-views', labelKey: 'saved_views', icon: Bookmark },
  { to: '/settings', labelKey: 'settings', icon: Settings },
];

const HOVER_IN_MS = 80;
const HOVER_OUT_MS = 300;

export function IconRail() {
  const { t } = useTranslation('nav');
  const [open, setOpen] = React.useState(false);
  const openTimer = React.useRef<number | null>(null);
  const closeTimer = React.useRef<number | null>(null);

  const cancel = (h: React.MutableRefObject<number | null>) => {
    if (h.current !== null) {
      window.clearTimeout(h.current);
      h.current = null;
    }
  };

  const handleEnter = () => {
    cancel(closeTimer);
    if (open) return;
    openTimer.current = window.setTimeout(() => setOpen(true), HOVER_IN_MS);
  };

  const handleLeave = () => {
    cancel(openTimer);
    closeTimer.current = window.setTimeout(() => setOpen(false), HOVER_OUT_MS);
  };

  const handleFocus = () => setOpen(true);

  return (
    <>
      {/* 8px hover hot zone always present on the left edge */}
      <div
        className="fixed left-0 top-strip bottom-0 w-2 z-30"
        onMouseEnter={handleEnter}
        onMouseLeave={handleLeave}
        aria-hidden
      />
      <nav
        aria-label={t('primary_navigation')}
        data-open={open}
        onMouseEnter={handleEnter}
        onMouseLeave={handleLeave}
        onFocus={handleFocus}
        className={cn(
          'fixed left-0 top-strip bottom-0 z-40 flex w-rail flex-col items-center gap-1 border-r border-border bg-surface py-3 transition-transform duration-150',
          open ? 'translate-x-0' : '-translate-x-full',
        )}
      >
        {ITEMS.map((item) => {
          const label = t(item.labelKey);
          return (
            <Tooltip key={item.to}>
              <TooltipTrigger asChild>
                <NavLink
                  to={item.to}
                  className={({ isActive }) =>
                    cn(
                      'flex h-9 w-9 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground',
                      isActive && 'bg-accent-bg text-accent',
                    )
                  }
                >
                  <item.icon className="h-5 w-5" />
                  <span className="sr-only">{label}</span>
                </NavLink>
              </TooltipTrigger>
              <TooltipContent side="right">{label}</TooltipContent>
            </Tooltip>
          );
        })}
      </nav>
    </>
  );
}
