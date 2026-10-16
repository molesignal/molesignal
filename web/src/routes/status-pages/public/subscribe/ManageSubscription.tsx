import { useMutation, useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';

import * as statusPagesApi from '@/api/statusPages';
import type { PublicStatusPageComponent } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';

import { namedComponents } from '../../model';
import { PUBLIC_CONTROL_HEIGHT } from '../control';
import { chosenComponentIds } from './actions';
import { ComponentChoice } from './ComponentChoice';

/**
 * A subscriber's own subscription, opened from the manage link every message
 * carries: what it reaches, what it hears about, and the way to stop it.
 */
export function ManageSubscription({
  slug,
  token,
  components,
  brandColor,
  copy,
  onClose,
}: {
  slug: string;
  token: string;
  components: PublicStatusPageComponent[];
  brandColor: string;
  copy: TFunction;
  onClose: () => void;
}) {
  const preferences = useQuery({
    queryKey: ['status-pages', 'public', slug, 'preferences', token],
    queryFn: () => statusPagesApi.readSubscriptionPreferences(slug, token),
    retry: false,
  });
  const [scope, setScope] = React.useState<'all' | 'selected'>('all');
  const [chosen, setChosen] = React.useState<string[]>([]);
  const [done, setDone] = React.useState<'saved' | 'unsubscribed' | null>(null);

  React.useEffect(() => {
    if (!preferences.data) return;
    setScope(preferences.data.component_scope);
    setChosen(preferences.data.component_ids);
  }, [preferences.data]);

  const save = useMutation({
    mutationFn: () =>
      statusPagesApi.updateSubscriptionPreferences(slug, token, chosenComponentIds(scope, chosen)),
    onSuccess: () => setDone('saved'),
  });
  const unsubscribe = useMutation({
    mutationFn: () => statusPagesApi.unsubscribePublic(slug, token),
    onSuccess: () => setDone('unsubscribed'),
  });
  const error = save.error ?? unsubscribe.error;
  const subscription = preferences.data;
  const stopped = subscription?.status === 'unsubscribed' || done === 'unsubscribed';
  const busy = save.isPending || unsubscribe.isPending;

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        data-theme="light"
        style={{ '--status-brand': brandColor } as React.CSSProperties}
        className="w-[min(520px,calc(100vw-24px))] gap-0 border-bd-1 bg-white p-0 text-tx-1"
      >
        <DialogHeader className="border-b border-bd-0 px-5 pb-5 pt-6 sm:px-6">
          <DialogTitle className="text-xl font-display-strong text-tx-0">
            {copy('public.subscription.manage_title')}
          </DialogTitle>
          <DialogDescription className="mt-2 text-sm leading-6 text-tx-2">
            {subscription
              ? copy('public.subscription.manage_description', {
                  target: subscription.target_label ?? subscription.masked_target,
                  channel: copy(`public.subscription.${subscription.channel}`),
                })
              : copy('public.subscription.manage_loading')}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-5 px-5 py-6 sm:px-6">
          {preferences.isError ? (
            <p role="alert" className="text-sm leading-6 text-tx-2">{copy('public.subscription.action_failed')}</p>
          ) : stopped ? (
            <p role="status" className="text-sm leading-6 text-tx-1">{copy('public.subscription.action_unsubscribed')}</p>
          ) : subscription ? (
            <>
              {namedComponents(components).length >= 2 ? (
                <ComponentChoice
                  components={components}
                  scope={scope}
                  chosen={chosen}
                  onScopeChange={(next) => {
                    setScope(next);
                    setDone(null);
                  }}
                  onChosenChange={(next) => {
                    setChosen(next);
                    setDone(null);
                  }}
                  copy={copy}
                  disabled={busy}
                />
              ) : (
                <p className="text-sm leading-6 text-tx-2">{copy('public.subscription.components_all_hint')}</p>
              )}
              {subscription.status === 'pending' && (
                <p className="rounded-md bg-bg-1 px-3 py-2 text-sm leading-6 text-tx-2">
                  {copy('public.subscription.manage_pending')}
                </p>
              )}
              {done === 'saved' && (
                <p role="status" className="text-sm text-tx-1">{copy('public.subscription.manage_saved')}</p>
              )}
              {error && (
                <p role="alert" className="rounded-md bg-red-dim px-3 py-2 text-sm text-red">
                  {toApiError(error).message}
                </p>
              )}
            </>
          ) : null}

          {subscription && !stopped && (
            <DialogFooter className="items-center justify-between gap-3 space-x-0 pt-1">
              <Button
                type="button"
                variant="outline"
                disabled={busy}
                className={cn(PUBLIC_CONTROL_HEIGHT, 'px-3.5')}
                onClick={() => unsubscribe.mutate()}
              >
                {copy('public.subscription.manage_unsubscribe')}
              </Button>
              {namedComponents(components).length >= 2 && (
                <Button
                  type="button"
                  size="lg"
                  disabled={busy || (scope === 'selected' && chosen.length === 0)}
                  className={cn(PUBLIC_CONTROL_HEIGHT, 'bg-[var(--status-brand)] text-white hover:brightness-95')}
                  onClick={() => save.mutate()}
                >
                  {copy('public.subscription.manage_save')}
                </Button>
              )}
            </DialogFooter>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
