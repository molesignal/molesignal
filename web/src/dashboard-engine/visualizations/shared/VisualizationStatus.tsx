import { useDashboardText } from '../../i18n';
import type { PanelData } from '../../schema';

export type VisualizationStatusKind = 'loading' | 'error';

export function visualizationStatusKind(
  data: PanelData,
): VisualizationStatusKind | null {
  if (data.state === 'error') return 'error';
  if (data.frames.length === 0 && data.state === 'loading') {
    return 'loading';
  }
  return null;
}

export function VisualizationStatus({
  kind,
  detail,
}: {
  kind: VisualizationStatusKind;
  detail?: string | undefined;
}) {
  const tr = useDashboardText();
  const error = kind === 'error';
  return (
    <div
      role={error ? 'alert' : 'status'}
      aria-live={error ? 'assertive' : 'polite'}
      className="grid h-full min-h-20 place-items-center px-4 text-center font-sans"
    >
      <div className="min-w-0">
        <div className={error ? 'text-xs text-red' : 'text-xs text-tx-3'}>
          {tr(error ? 'Unable to load visualization' : 'Loading visualization…')}
        </div>
        {error && detail && (
          <div
            className="mt-1 max-w-md truncate font-mono text-type-micro text-tx-3"
            title={detail}
          >
            {detail}
          </div>
        )}
      </div>
    </div>
  );
}
