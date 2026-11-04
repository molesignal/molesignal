import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type {
  ComponentDependency,
  DependencyHealth,
  DependencyKind,
  DependencyMode,
} from '@/api/statusPages';
import { Pill } from '@/shell/chrome';
import { FieldArray, FormRadio, FormSection } from '@/shell/FormDrawer';

import { DependencyRow } from './DependencyRow';
import { dependencyHref } from './model';
import { useDependencyTargets } from './targets';
import { COMPONENT_TONE, componentStatusLabel } from '../../../model';
import { NotifySwitch } from '../../NotifySwitch';

export interface DependencyDraft {
  dependencies: ComponentDependency[];
  mode: DependencyMode;
  /** Whether a status the component follows tells subscribers. */
  notify: boolean;
}

/**
 * What the component depends on, and how the health worked out from that
 * reaches it: the component follows it, or the page's automation rules
 * publish incidents from it.
 */
export function DependencyFields({
  draft,
  onChange,
  health,
}: {
  draft: DependencyDraft;
  onChange: (draft: DependencyDraft) => void;
  /** The health last worked out, for a component that already depends on something. */
  health: DependencyHealth | null | undefined;
}) {
  const { t } = useTranslation('status-pages');
  const targets = useDependencyTargets(true);

  return (
    <FormSection title={t('dependencies.title')} description={t('dependencies.hint')}>
      {health && <DependencyHealthNote health={health} label={targets.label} />}
      <FieldArray<ComponentDependency>
        items={draft.dependencies}
        onChange={(dependencies) => onChange({ ...draft, dependencies })}
        newItem={() => ({ kind: 'technical_service', target: '', impact: 'outage' })}
        addLabel={t('dependencies.add')}
        removeLabel={t('dependencies.remove')}
        emptyHint={t('dependencies.empty')}
        renderItem={(dependency, _, setItem) => (
          <DependencyRow dependency={dependency} onChange={setItem} options={targets.options} />
        )}
      />
      {draft.dependencies.length > 0 && (
        <>
          <FormRadio<DependencyMode>
            value={draft.mode}
            onChange={(mode) => onChange({ ...draft, mode })}
            options={(['follow', 'automation'] as const).map((mode) => ({
              value: mode,
              label: t(`dependencies.modes.${mode}`),
              hint: t(`dependencies.modes.${mode}_hint`),
            }))}
          />
          {draft.mode === 'follow' && (
            <NotifySwitch
              label={t('fields.notify_subscribers')}
              hint={t('dependencies.follow_notify_hint')}
              checked={draft.notify}
              onCheckedChange={(notify) => onChange({ ...draft, notify })}
            />
          )}
        </>
      )}
    </FormSection>
  );
}

/** The health worked out from the dependencies, and the ones that cause it. */
function DependencyHealthNote({
  health,
  label,
}: {
  health: DependencyHealth;
  label: (kind: DependencyKind, target: string) => string;
}) {
  const { t } = useTranslation('status-pages');

  return (
    <div data-dependency-health className="space-y-2 rounded-md bg-bg-1 px-3 py-2.5">
      <div className="flex items-center gap-2 text-sm text-tx-1">
        <span>{t('dependencies.health')}</span>
        <Pill tone={COMPONENT_TONE[health.status]}>{componentStatusLabel(t, health.status)}</Pill>
      </div>
      {health.causes.length > 0 && (
        <ul className="space-y-1 text-xs leading-5 text-tx-2">
          {health.causes.map((cause) => (
            <li key={`${cause.kind}:${cause.target}`} className="flex flex-wrap items-center gap-x-1.5">
              <span className="text-tx-3">{t(`dependencies.kinds.${cause.kind}`)}</span>
              <Link to={dependencyHref(cause.kind, cause.target)} className="text-tx-1 hover:underline">
                {label(cause.kind, cause.target)}
              </Link>
              <span>· {componentStatusLabel(t, cause.status)}</span>
              {cause.incidentCount > 0 && (
                <span>· {t('dependencies.open_incidents', { count: cause.incidentCount })}</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
