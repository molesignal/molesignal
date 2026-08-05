import { Compass } from 'lucide-react';
import { Trans } from 'react-i18next';

import { Kbd } from '@/shell/ui/kbd';
import { useInvestigationStack } from '@/stores/useInvestigationStack';

export function Investigate() {
  const frames = useInvestigationStack((s) => s.frames);

  if (frames.length === 0) {
    return (
      <div className="flex h-[calc(100vh-32px)] flex-col items-center justify-center gap-3 text-muted-foreground">
        <Compass className="h-8 w-8" />
        <div className="text-sm">
          <Trans
            i18nKey="shell:pages.investigate.empty_title"
            components={{ kbd: <Kbd /> }}
          />
        </div>
        <div className="text-xs">
          <Trans
            i18nKey="shell:pages.investigate.empty_more"
            components={{ kbd: <Kbd />, kbdSm: <Kbd size="sm" /> }}
          />
        </div>
      </div>
    );
  }
  // Root frame (frames[0]) renders in main; higher frames are drawer-stacked elsewhere.
  return <div className="h-[calc(100vh-32px)]" id="investigate-root" data-frame-id={frames[0]!.id} />;
}
