import { useDashboardText } from '../../i18n';

export function EmptyVisualization({ label }: { label?: string }) {
  const tr = useDashboardText();
  return (
    <div className="grid h-full min-h-20 place-items-center font-sans text-xs text-tx-3">
      {label ?? tr('No data')}
    </div>
  );
}
