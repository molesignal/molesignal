import { Trans } from 'react-i18next';

import { Kbd } from '@/shell/ui/kbd';

/**
 * Minimal placeholder route body. Each route gets a stub so the router and the
 * IconRail compile end-to-end; the real bodies arrive with the feature module
 * that owns them (Services with topology, Dashboards with the native viewer, etc.).
 */
export function Placeholder({ title }: { title: string }) {
  return (
    <div className="flex h-[calc(100vh-32px)] flex-col items-center justify-center gap-2 text-muted-foreground">
      <div className="text-base font-semibold text-foreground">{title}</div>
      <div className="text-xs">
        <Trans
          i18nKey="shell:pages.placeholder.navigate_hint"
          components={{ kbd: <Kbd /> }}
        />
      </div>
    </div>
  );
}
