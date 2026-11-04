import { UserRound } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ScheduledPipeline } from '@/api/pipelines';
import { useActionAccess } from '@/product/actionAccess';
import { useAuthStore } from '@/stores/auth';

import { auditReaderName } from '../auditReader';

/**
 * Beside an `_audit` source: whose access the runs read it with, what saving does to that, and what
 * happens when they lose the permission. `pipeline` is what was last saved, `null` for a new one:
 * it reads as nobody yet, and as whoever saves it from then on. Saving changes hands only for someone
 * who holds `audit.read` themselves, and only someone who can save is told about it.
 */
export function AuditReaderNote({
  pipeline,
  canSave,
}: {
  pipeline: ScheduledPipeline | null;
  canSave: boolean;
}) {
  const { t } = useTranslation('pipelines');
  const me = useAuthStore((state) => state.ctx);
  const holdsAuditRead = useActionAccess({ permission: 'audit.read' }).allowed;
  const reader = pipeline?.auditReaderId;
  const other = Boolean(reader) && reader !== me?.userId;
  const name = other
    ? auditReaderName(pipeline)
    : t('editor.audit_reader.you', {
        name: auditReaderName(pipeline) || me?.displayName || me?.email || me?.userId,
      });

  return (
    <div className="mt-1 grid gap-0.5 font-sans text-xs leading-relaxed text-tx-3" data-audit-reader>
      <p className="m-0 flex min-w-0 flex-wrap items-center gap-x-1.5">
        <UserRound aria-hidden className="h-3.5 w-3.5 shrink-0" />
        <span>{t('editor.audit_reader.label')}</span>{' '}
        <span
          className="min-w-0 break-words font-strong text-tx-0"
          // The email tells two people with the same name apart.
          title={other && pipeline?.auditReader?.displayName ? pipeline.auditReader.email : undefined}
        >
          {name}
        </span>
        {other && canSave && holdsAuditRead && (
          <>
            {' '}
            <span className="text-tx-1">{t('editor.audit_reader.takeover')}</span>
          </>
        )}
      </p>
      <p className="m-0" data-audit-source-note>
        {t('editor.source_audit_note')}
      </p>
    </div>
  );
}
