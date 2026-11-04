import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowDown, ArrowUp, Pencil, Plus, Trash2, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import type { ErrorRule, ErrorRuleWrite, ErrorSummary } from '@/api/apm';
import { apmErrorRulesApi, apmErrorRulesQueryKeys } from '@/api/apmErrorRules';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetTitle } from '@/shell/ui/sheet';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import { ErrorRuleForm } from './ErrorRuleForm';
import { PATTERN_FIELD, RULE_PATTERNS, ruleWrite } from './rules';
import { ApmQueryState } from '../frame/Page';

/** What the sheet shows: the rules, or one rule being set. */
export type ErrorRulesView = { mode: 'list' } | { mode: 'form'; rule: ErrorRule | null; initial: ErrorRuleWrite };

const ICON_BUTTON =
  'inline-flex h-7 w-7 items-center justify-center rounded-md bg-[var(--control-surface)] text-tx-2 outline-none hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 disabled:opacity-40';

/**
 * The organization's error grouping rules in the order they are tried, each
 * moved up or down, changed or deleted in place; a new one, or one being
 * changed, opens as a form in the same sheet.
 */
export function ErrorRulesSheet({
  view,
  onView,
  groups,
  newRule,
}: {
  view: ErrorRulesView | null;
  onView: (view: ErrorRulesView | null) => void;
  /** The error groups in view, for telling how many a rule would take. */
  groups: readonly ErrorSummary[];
  /** What a new rule starts from. */
  newRule: () => ErrorRuleWrite;
}) {
  const { t } = useTranslation('apm');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const form = view?.mode === 'form' ? view : null;

  return (
    <Sheet open={view !== null} onOpenChange={(next) => !next && onView(null)}>
      {/* The close button sits in the header row beside the action, not over it. */}
      <SheetContent side="right" hideClose className="flex w-[520px] max-w-full flex-col gap-0 p-0" data-apm-error-rules="">
        <div className="flex items-start gap-3 border-b border-bd-0 px-4 py-3">
          <div className="min-w-0 flex-1">
            <SheetTitle className="font-sans text-sm font-strong text-tx-0">
              {form ? t(form.rule ? 'errors.rules.edit_title' : 'errors.rules.new_title') : t('errors.rules.title')}
            </SheetTitle>
            <SheetDescription className="mt-0.5 font-sans text-xs text-tx-2">{t('errors.rules.description')}</SheetDescription>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            {!form && (
              <Button type="button" size="sm" variant="outline" onClick={() => onView({ mode: 'form', rule: null, initial: newRule() })} data-apm-error-rule-new="">
                <Plus aria-hidden className="h-3.5 w-3.5" />
                {t('errors.rules.new')}
              </Button>
            )}
            <SheetClose asChild>
              <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('drawer.close')}>
                <X className="h-4 w-4" />
              </Button>
            </SheetClose>
          </div>
        </div>
        {/* One form per opening: its fields start from the rule each time. */}
        {form ? (
          <ErrorRuleForm key={form.rule?.id ?? 'new'} orgId={orgId} rule={form.rule} initial={form.initial} groups={groups} onDone={() => onView({ mode: 'list' })} />
        ) : (
          view && <RuleList orgId={orgId} onEdit={(rule) => onView({ mode: 'form', rule, initial: ruleWrite(rule) })} />
        )}
      </SheetContent>
    </Sheet>
  );
}

function RuleList({ orgId, onEdit }: { orgId: string; onEdit: (rule: ErrorRule) => void }) {
  const { t } = useTranslation('apm');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'apm.error_rules.manage' });
  const [removing, setRemoving] = React.useState<ErrorRule | null>(null);
  const rules = useQuery({
    queryKey: apmErrorRulesQueryKeys.list(orgId),
    queryFn: () => apmErrorRulesApi.list(),
    enabled: Boolean(orgId),
  });
  const listed = rules.data ?? [];

  const order = useMutation({
    mutationFn: (ids: string[]) => apmErrorRulesApi.order(ids),
    onSuccess: (ordered) => queryClient.setQueryData(apmErrorRulesQueryKeys.list(orgId), ordered),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const remove = useMutation({
    mutationFn: (rule: ErrorRule) => apmErrorRulesApi.remove(rule.id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: apmErrorRulesQueryKeys.list(orgId) });
      toast.success(t('errors.rules.removed'));
      setRemoving(null);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const move = (index: number, by: -1 | 1) => {
    const ids = listed.map((rule) => rule.id);
    const [moved] = ids.splice(index, 1);
    if (moved === undefined) return;
    ids.splice(index + by, 0, moved);
    order.mutate(ids);
  };
  const busy = access.disabled || order.isPending;

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <ApmQueryState query={rules} empty={listed.length === 0} emptyTitle={t('errors.rules.none')} compact>
        <ol aria-label={t('errors.rules.title')}>
          {listed.map((rule, index) => (
            <li key={rule.id} data-apm-error-rule={rule.id} className="flex items-start gap-3 border-b border-bd-0 px-4 py-2.5 font-sans text-xs">
              <span className="w-5 shrink-0 pt-px text-right tabular-nums text-tx-3">{index + 1}</span>
              <span className="min-w-0 flex-1 space-y-0.5">
                <span className="flex min-w-0 items-center gap-2">
                  <span className="truncate font-strong text-tx-0">{rule.name}</span>
                  <span className={cn('shrink-0 font-strong', rule.action === 'merge' ? 'text-[var(--blue-ink)]' : 'text-tx-2')}>{t(`errors.rules.action.${rule.action}`)}</span>
                  {!rule.enabled && <span className="shrink-0 text-tx-3">{t('errors.rules.disabled')}</span>}
                </span>
                <span className="block truncate text-tx-2">
                  {[rule.serviceName || t('errors.rules.every_service'), ...RULE_PATTERNS.filter((pattern) => rule[PATTERN_FIELD[pattern]]).map((pattern) => t(`errors.rules.pattern.${pattern}`, { pattern: rule[PATTERN_FIELD[pattern]] }))].join(' · ')}
                </span>
              </span>
              <span className="flex shrink-0 items-center gap-1" title={access.reason}>
                <button type="button" className={ICON_BUTTON} disabled={busy || index === 0} onClick={() => move(index, -1)} aria-label={t('errors.rules.move_up', { name: rule.name })}>
                  <ArrowUp aria-hidden className="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  className={ICON_BUTTON}
                  disabled={busy || index === listed.length - 1}
                  onClick={() => move(index, 1)}
                  aria-label={t('errors.rules.move_down', { name: rule.name })}
                >
                  <ArrowDown aria-hidden className="h-3.5 w-3.5" />
                </button>
                <button type="button" className={ICON_BUTTON} disabled={access.disabled} onClick={() => onEdit(rule)} aria-label={t('errors.rules.edit', { name: rule.name })}>
                  <Pencil aria-hidden className="h-3.5 w-3.5" />
                </button>
                <button type="button" className={ICON_BUTTON} disabled={access.disabled} onClick={() => setRemoving(rule)} aria-label={t('errors.rules.remove', { name: rule.name })}>
                  <Trash2 aria-hidden className="h-3.5 w-3.5" />
                </button>
              </span>
            </li>
          ))}
        </ol>
      </ApmQueryState>
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('errors.rules.remove_title', { name: removing?.name ?? '' })}
        description={t('errors.rules.remove_description')}
        confirmLabel={t('errors.rules.remove_confirm')}
        cancelLabel={t('actions.cancel')}
        busy={remove.isPending}
        busyLabel={t('errors.rules.removing')}
        onConfirm={() => removing && remove.mutate(removing)}
      />
    </div>
  );
}
