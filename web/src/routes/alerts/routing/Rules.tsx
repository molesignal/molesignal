import { useMutation, useQueryClient } from '@tanstack/react-query';
import { GripVertical, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as routingApi from '@/api/alertRouting';
import type { RoutingRule } from '@/api/alertRouting';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { IconButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { useFlipList } from '@/shell/polish/useFlipList';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import { rangesLabel } from '@/shell/weekly-ranges/model';

import { conditionText, dropped, moved, ROUTING_RULES_KEY, ROUTING_KEY, ruleInput, targetExists } from './model';
import { SourceSection } from '../sources/Section';

interface DropTarget {
  id: string;
  position: 'before' | 'after';
}

/**
 * The routing rules in the order they are tried: the first an incident matches decides where it
 * goes. A rule is moved by dragging it or, from its handle, with the arrow keys; it slides to its
 * place and the new order is saved at once.
 */
export function RoutingRules({
  rules,
  policies,
  teams,
  onEdit,
}: {
  rules: readonly RoutingRule[];
  policies: ReadonlyArray<{ id: string; name: string }>;
  teams: ReadonlyArray<{ id: string; name: string }>;
  onEdit: (rule: RoutingRule) => void;
}) {
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'alerts.routing.manage' });
  const [pendingOrder, setPendingOrder] = React.useState<string[] | null>(null);
  const [dragging, setDragging] = React.useState<string | null>(null);
  const [dropTarget, setDropTarget] = React.useState<DropTarget | null>(null);
  const [removing, setRemoving] = React.useState<RoutingRule | null>(null);
  const listRef = React.useRef<HTMLOListElement>(null);

  const byId = new Map(rules.map((rule) => [rule.id, rule]));
  const ids = pendingOrder ?? rules.map((rule) => rule.id);
  const ordered = ids.flatMap((id) => byId.get(id) ?? []);
  const capture = useFlipList(listRef, ids.join('\n'));
  const policyNames = new Map(policies.map((policy) => [policy.id, policy.name]));
  const teamNames = new Map(teams.map((team) => [team.id, team.name]));

  const reorder = useMutation({
    mutationFn: (next: string[]) => routingApi.reorderRules(next),
    onMutate: (next) => {
      capture();
      setPendingOrder(next);
    },
    onSuccess: (saved) => queryClient.setQueryData(ROUTING_RULES_KEY, saved),
    onError: (error) => {
      // The rules slide back to where they were saved.
      capture();
      toast.error(toApiError(error).message);
    },
    onSettled: () => setPendingOrder(null),
  });
  const toggle = useMutation({
    mutationFn: (rule: RoutingRule) => routingApi.updateRule(rule.id, { ...ruleInput(rule), enabled: !rule.enabled }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ROUTING_KEY }),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const remove = useMutation({
    mutationFn: (id: string) => routingApi.removeRule(id),
    onSuccess: () => {
      toast.success(t('routing.toast_deleted'));
      void queryClient.invalidateQueries({ queryKey: ROUTING_KEY });
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const canReorder = manageAccess.allowed && ordered.length > 1 && !reorder.isPending;

  const save = (next: string[]) => {
    if (next.join('\n') !== ids.join('\n')) reorder.mutate(next);
  };

  const moveByKey = (id: string, index: number, step: -1 | 1) => {
    if (!canReorder || index + step < 0 || index + step >= ids.length) return;

    save(moved(ids, index, index + step));
    // A row the list moved can lose focus on the way; the handle keeps it so the next key moves it again.
    window.requestAnimationFrame(() =>
      listRef.current?.querySelector<HTMLElement>(`[data-flip-key="${CSS.escape(id)}"] [data-rule-handle]`)?.focus(),
    );
  };

  const endDrag = () => {
    setDragging(null);
    setDropTarget(null);
  };

  const summary = (rule: RoutingRule) =>
    [
      ...(rule.conditions.length > 0 ? rule.conditions.map(conditionText) : [t('routing.every_incident')]),
      rule.minSeverity ? t('routing.min_severity', { severity: t(`severity.${rule.minSeverity}`) }) : null,
      rule.origins.length > 0
        ? t('routing.only_origins', { origins: rule.origins.map((origin) => t(`routing.origins.${origin}`)).join(t('routing.list_separator')) })
        : null,
      rule.priorities && rule.priorities.length > 0
        ? t('routing.only_priorities', { priorities: rule.priorities.join(t('routing.list_separator')) })
        : null,
      rule.time
        ? t(`routing.time_${rule.time.mode}`, {
            ranges: rangesLabel(rule.time.ranges, i18n.language),
            timezone: rule.time.timezone,
          })
        : null,
    ]
      .filter(Boolean)
      .join(' · ');

  return (
    <>
      <SourceSection title={t('routing.rules_title')} description={t('routing.rules_description')}>
        {ordered.length === 0 ? (
          <p className="m-0 px-[20px] pb-[20px] font-sans text-sm text-tx-2">{t('routing.rules_empty')}</p>
        ) : (
          <ol ref={listRef} data-routing-rules className="m-0 list-none divide-y divide-bd-0 border-t border-bd-0 p-0">
            {ordered.map((rule, index) => {
              const target = dropTarget?.id === rule.id ? dropTarget : null;
              const exists = targetExists(rule.target, new Set(policyNames.keys()), new Set(teamNames.keys()));
              const enabled = toggle.isPending && toggle.variables?.id === rule.id ? !rule.enabled : rule.enabled;
              const targetName =
                rule.target.kind === 'policy' ? policyNames.get(rule.target.policyId) : teamNames.get(rule.target.teamId);

              return (
                <li
                  key={rule.id}
                  data-flip-key={rule.id}
                  data-routing-rule={rule.name}
                  draggable={canReorder}
                  onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = 'move';
                    event.dataTransfer.setData('text/plain', rule.id);
                    setDragging(rule.id);
                  }}
                  onDragOver={(event) => {
                    if (!dragging || dragging === rule.id) return;

                    event.preventDefault();
                    event.dataTransfer.dropEffect = 'move';
                    const box = event.currentTarget.getBoundingClientRect();
                    setDropTarget({ id: rule.id, position: event.clientY < box.top + box.height / 2 ? 'before' : 'after' });
                  }}
                  onDrop={(event) => {
                    event.preventDefault();

                    if (dragging && target) save(dropped(ids, dragging, rule.id, target.position));
                    endDrag();
                  }}
                  onDragEnd={endDrag}
                  className={cn(
                    'relative flex flex-wrap items-center gap-x-3 gap-y-1.5 bg-[var(--functional-surface)] py-[10px] pl-[12px] pr-[20px]',
                    dragging === rule.id && 'opacity-40',
                    target?.position === 'before' &&
                      'before:absolute before:inset-x-[20px] before:-top-px before:h-0.5 before:rounded-full before:bg-indigo',
                    target?.position === 'after' &&
                      'after:absolute after:inset-x-[20px] after:-bottom-px after:h-0.5 after:rounded-full after:bg-indigo',
                  )}
                >
                  <button
                    type="button"
                    data-rule-handle
                    aria-label={t('routing.reorder', { name: rule.name })}
                    title={t('routing.reorder_hint')}
                    disabled={!canReorder}
                    onKeyDown={(event) => {
                      if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;

                      event.preventDefault();
                      moveByKey(rule.id, index, event.key === 'ArrowUp' ? -1 : 1);
                    }}
                    className="grid h-8 w-5 shrink-0 cursor-grab place-items-center rounded-sm text-tx-3 outline-none hover:bg-bg-2 hover:text-tx-1 active:cursor-grabbing disabled:cursor-default disabled:opacity-60"
                  >
                    <GripVertical aria-hidden className="h-4 w-4" />
                  </button>
                  <span aria-hidden className="w-5 shrink-0 text-right font-mono text-xs tabular-nums text-tx-3">
                    {index + 1}
                  </span>
                  <div className="min-w-[200px] flex-1">
                    <button
                      type="button"
                      onClick={() => onEdit(rule)}
                      className={cn(
                        'block max-w-full truncate text-left font-sans text-sm font-strong underline-offset-4 outline-none hover:underline focus-visible:underline',
                        enabled ? 'text-tx-0' : 'text-tx-2',
                      )}
                    >
                      {rule.name}
                    </button>
                    <div className="mt-0.5 truncate font-sans text-xs text-tx-2">{summary(rule)}</div>
                  </div>
                  <div className="w-[220px] min-w-0 shrink-0 font-sans text-sm" data-routing-target>
                    {exists ? (
                      <span className="flex min-w-0 items-baseline gap-1.5">
                        <span className="shrink-0 text-xs text-tx-2">
                          {t(rule.target.kind === 'policy' ? 'routing.target_policy' : 'routing.target_team')}
                        </span>
                        <span className="truncate text-tx-0">{targetName}</span>
                      </span>
                    ) : (
                      <span className="block truncate text-[var(--red-ink)]" title={t('routing.target_missing_hint')}>
                        {t('routing.target_missing')}
                      </span>
                    )}
                  </div>
                  <Switch
                    checked={enabled}
                    disabled={manageAccess.disabled || toggle.isPending}
                    disabledReason={manageAccess.reason}
                    onCheckedChange={() => toggle.mutate(rule)}
                    aria-label={t('routing.enable', { name: rule.name })}
                  />
                  <IconButton
                    aria-label={t('routing.delete_named', { name: rule.name })}
                    title={tc('actions.delete')}
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    className="enabled:hover:text-red-soft"
                    onClick={() => setRemoving(rule)}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </IconButton>
                </li>
              );
            })}
          </ol>
        )}
      </SourceSection>
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('routing.delete_title')}
        description={t('routing.delete_description', { name: removing?.name ?? '' })}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) remove.mutate(removing.id);
        }}
      />
    </>
  );
}
