import { Bell, LayoutDashboard, Network, Siren, Waves, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { MessageReference, ReferenceKind } from '@/api/agent/chat';

export const REFERENCE_ICON: Record<ReferenceKind, typeof Bell> = {
  incident: Siren,
  alert_rule: Bell,
  service: Network,
  dashboard: LayoutDashboard,
  stream: Waves,
};

/** One attached reference: what kind it is and its name. */
export function ReferenceChip({
  reference,
  onRemove,
}: {
  reference: MessageReference;
  onRemove?: () => void;
}) {
  const { t } = useTranslation('agent');
  const Icon = REFERENCE_ICON[reference.kind];
  const kind = t(`references.kinds.${reference.kind}`);
  return (
    <span
      data-agent-reference={`${reference.kind}:${reference.id}`}
      className="inline-flex h-7 max-w-[260px] items-center gap-1 rounded-md bg-indigo/10 px-2 font-sans type-micro text-tx-1"
    >
      <Icon aria-hidden="true" className="h-3 w-3 shrink-0 text-indigo" />
      <span className="shrink-0 text-tx-3">{kind}</span>
      <span className="truncate font-strong">{reference.label || reference.id}</span>
      {onRemove && (
        <button
          type="button"
          aria-label={t('references.remove', { name: reference.label || reference.id })}
          onClick={onRemove}
          className="-mr-1 grid h-5 w-5 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0"
        >
          <X className="h-3 w-3" />
        </button>
      )}
    </span>
  );
}

/** The references attached to a question, under its text. */
export function ReferenceList({ references }: { references: readonly MessageReference[] }) {
  if (references.length === 0) return null;
  return (
    <div className="mt-1.5 flex flex-wrap justify-end gap-1">
      {references.map((reference) => (
        <ReferenceChip key={`${reference.kind}:${reference.id}`} reference={reference} />
      ))}
    </div>
  );
}
