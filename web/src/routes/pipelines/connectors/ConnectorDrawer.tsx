import { useMutation, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { ChevronLeft } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as connectorsApi from '@/api/connectors';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer, FormSubmitFooter } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { ConnectorForm } from './ConnectorForm';
import { KIND_ICONS } from './kindIcons';
import {
  configOf,
  draftOf,
  draftProblem,
  KINDS,
  newDraft,
  type ConnectorDraft,
  type ConnectorKind,
  type Direction,
  type DraftProblem,
} from './kinds';
import { PushSetup } from './PushSetup';
import { newPushToken } from './pushToken';

/** What the drawer is open for: a new connector, perhaps of a known direction, or a saved one. */
export type DrawerTarget =
  | { mode: 'new'; direction?: Direction }
  | { mode: 'edit'; connector: connectorsApi.Connector };

const FORM_ID = 'connector-form';

/**
 * Creates or changes a connector. A new one starts by saying what it connects
 * to; the settings follow from that. A push connector ends on what to give the
 * other service, since its token can be shown only once.
 */
export function ConnectorDrawer({
  target,
  access,
  onClose,
}: {
  target: DrawerTarget | null;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('pipelines');
  const queryClient = useQueryClient();
  const saved = target?.mode === 'edit' ? target.connector : undefined;
  const [draft, setDraft] = React.useState<ConnectorDraft | null>(null);
  /** The token just saved for a push connector, to be handed over once. */
  const [issued, setIssued] = React.useState<{ kind: ConnectorKind; token: string } | null>(null);

  // Each opening starts from what it was opened for, not from what the last one left.
  React.useEffect(() => {
    setIssued(null);
    setDraft(target?.mode === 'edit' ? draftOf(target.connector) : null);
  }, [target]);

  const problem = draft ? draftProblem(draft, saved) : null;
  const save = useMutation({
    mutationFn: async (current: ConnectorDraft) => {
      const payload = {
        name: current.name.trim(),
        kind: current.kind,
        config_json: configOf(current, saved),
        enabled: current.enabled,
      };

      if (saved) await connectorsApi.update(saved.id, payload);
      else await connectorsApi.create(payload);

      return current;
    },
    onSuccess: (current) => {
      toast.success(t(saved ? 'connectors.toast.updated' : 'connectors.toast.created'));
      void queryClient.invalidateQueries({ queryKey: ['connectors'] });

      // A token that was just saved is handed over now: it cannot be read back later.
      const token = current.auth === 'push_token' ? (current.values.push_token ?? '') : '';
      if (token) setIssued({ kind: current.kind, token });
      else onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const title = issued
    ? t('connectors.push.title', { kind: t(`connectors.kinds.${issued.kind}`) })
    : saved
      ? t('connectors.drawer.edit_title', { name: saved.name })
      : t('connectors.drawer.new_title');

  return (
    <FormDrawer
      open={target !== null}
      onOpenChange={(open) => !open && onClose()}
      title={title}
      width={560}
      footer={
        issued ? (
          <div className="flex justify-end">
            <ChromeButton variant="primary" onClick={onClose}>
              {t('connectors.push.done')}
            </ChromeButton>
          </div>
        ) : draft ? (
          <FormSubmitFooter
            busy={save.isPending}
            disabled={access.disabled}
            invalid={problem !== null}
            disabledReason={access.disabled ? access.reason : problem ? problemText(problem, t) : undefined}
            onCancel={onClose}
            submitLabel={t(saved ? 'connectors.drawer.save' : 'connectors.drawer.create')}
            formId={FORM_ID}
          />
        ) : undefined
      }
    >
      {issued ? (
        <PushSetup kind={issued.kind} token={issued.token} />
      ) : !draft ? (
        <KindPicker
          {...(target?.mode === 'new' && target.direction ? { first: target.direction } : {})}
          onPick={(kind) => setDraft(startDraft(kind))}
        />
      ) : (
        <form
          id={FORM_ID}
          onSubmit={(event) => {
            event.preventDefault();
            if (access.allowed && !problem && !save.isPending) save.mutate(draft);
          }}
        >
          <fieldset disabled={access.disabled || save.isPending} className="m-0 grid min-w-0 gap-[20px] border-0 p-0">
            <KindHeader kind={draft.kind} {...(saved ? {} : { onChange: () => setDraft(null) })} />
            <ConnectorForm draft={draft} saved={saved} onChange={setDraft} />
          </fieldset>
        </form>
      )}
    </FormDrawer>
  );
}

/**
 * A new connector of a kind. A push connector is found by its token, so it has
 * one from the start; it is shown once, after the connector is saved.
 */
function startDraft(kind: ConnectorKind): ConnectorDraft {
  const draft = newDraft(kind);

  return draft.auth === 'push_token' ? { ...draft, values: { push_token: newPushToken() } } : draft;
}

function problemText(problem: DraftProblem, t: TFunction<'pipelines'>): string {
  if (problem.code === 'field_missing' || problem.code === 'secret_missing') {
    return t(`connectors.problems.${problem.code}`, { field: t(`connectors.fields.${problem.key}`) });
  }

  return t(`connectors.problems.${problem.code}`);
}

/** What the connector is of, at the head of its settings. A new one can still be another kind. */
function KindHeader({ kind, onChange }: { kind: ConnectorKind; onChange?: () => void }) {
  const { t } = useTranslation('pipelines');
  const Icon = KIND_ICONS[kind];

  return (
    <div data-connector-kind={kind} className="flex items-start gap-[12px] rounded-md bg-[var(--control-surface)] px-[12px] py-[10px]">
      <span aria-hidden className="grid h-[28px] w-[28px] shrink-0 place-items-center rounded-md bg-[var(--functional-surface)] text-tx-2">
        <Icon className="h-3.5 w-3.5" strokeWidth={1.8} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block font-sans text-sm font-strong text-tx-0">{t(`connectors.kinds.${kind}`)}</span>
        <span className="block font-sans text-xs leading-relaxed text-tx-3">{t(`connectors.kind_hints.${kind}`)}</span>
      </span>
      {onChange && (
        <ChromeButton size="sm" variant="ghost" onClick={onChange} className="shrink-0 text-tx-2">
          <ChevronLeft aria-hidden className="h-3.5 w-3.5" />
          {t('connectors.drawer.change_kind')}
        </ChromeButton>
      )}
    </div>
  );
}

/** The kinds on offer, sources and destinations apart; `first` is the group asked for. */
function KindPicker({ first = 'source', onPick }: { first?: Direction; onPick: (kind: ConnectorKind) => void }) {
  const { t } = useTranslation('pipelines');
  const directions: Direction[] = first === 'source' ? ['source', 'destination'] : ['destination', 'source'];

  return (
    <div className="grid gap-[20px]">
      <p className="m-0 font-sans text-sm text-tx-2">{t('connectors.drawer.choose')}</p>
      {directions.map((direction) => (
        <section key={direction} aria-labelledby={`connector-kinds-${direction}`} className="grid gap-[6px]">
          <h3 id={`connector-kinds-${direction}`} className="m-0 font-sans text-xs font-strong text-tx-3">
            {t(`connectors.${direction}s.title`)}
          </h3>
          {KINDS.filter((spec) => spec.direction === direction).map((spec) => {
            const Icon = KIND_ICONS[spec.kind];

            return (
              <button
                key={spec.kind}
                type="button"
                data-connector-kind-option={spec.kind}
                onClick={() => onPick(spec.kind)}
                className="flex items-start gap-[12px] rounded-md bg-[var(--control-surface)] px-[12px] py-[10px] text-left outline-none transition-colors duration-fast hover:bg-bg-3 focus-visible:bg-bg-3"
              >
                <span aria-hidden className="grid h-[28px] w-[28px] shrink-0 place-items-center rounded-md bg-[var(--functional-surface)] text-tx-2">
                  <Icon className="h-3.5 w-3.5" strokeWidth={1.8} />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-baseline gap-[8px]">
                    <span className="font-sans text-sm font-strong text-tx-0">{t(`connectors.kinds.${spec.kind}`)}</span>
                    {/* Sources differ in who starts the transfer; every destination is delivered to. */}
                    {direction === 'source' && (
                      <span className="font-sans text-xs text-tx-3">{t(`connectors.transport.${spec.transport}`)}</span>
                    )}
                  </span>
                  <span className="block font-sans text-xs leading-relaxed text-tx-3">{t(`connectors.kind_hints.${spec.kind}`)}</span>
                </span>
              </button>
            );
          })}
        </section>
      ))}
    </div>
  );
}
