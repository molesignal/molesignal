import { useMutation } from '@tanstack/react-query';
import { BellPlus, Rss, Slack } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type {
  PublicStatusPageComponent,
  StatusPageLanguage,
  StatusPageSubscriberChannel,
} from '@/api/statusPages';
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
  DialogTrigger,
} from '@/shell/ui/dialog';
import { Input } from '@/shell/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import { PUBLIC_CONTROL_HEIGHT } from '../control';
import { chosenComponentIds, subscriptionAction } from './actions';
import { ComponentChoice } from './ComponentChoice';
import { ManageSubscription } from './ManageSubscription';

type ActionStatus =
  | 'confirming'
  | 'confirmed'
  | 'unsubscribed'
  | 'failed'
  | 'slack_connected'
  | 'slack_failed';

/**
 * The way to be told when a public status page changes: by email, by webhook,
 * in a Slack channel or over RSS, about the whole page or only the components
 * one relies on, confirmed before anything is sent.
 */
export function PublicStatusSubscribe({
  slug,
  language,
  brandColor,
  components,
  slackApp = false,
  privatePage = false,
}: {
  slug: string;
  language: StatusPageLanguage;
  /** The publisher's colour. The dialog is drawn outside the page, so it is handed its colour. */
  brandColor: string;
  components: PublicStatusPageComponent[];
  /** Whether the installation's Slack app can add the page to a channel. */
  slackApp?: boolean;
  privatePage?: boolean;
}) {
  const { i18n } = useTranslation('status-pages');
  const copy = i18n.getFixedT(language, 'status-pages');
  const location = useLocation();
  const navigate = useNavigate();
  const [open, setOpen] = React.useState(false);
  const [channel, setChannel] = React.useState<StatusPageSubscriberChannel>('email');
  const [target, setTarget] = React.useState('');
  const [scope, setScope] = React.useState<'all' | 'selected'>('all');
  const [chosen, setChosen] = React.useState<string[]>([]);
  const [accepted, setAccepted] = React.useState(false);
  const [actionStatus, setActionStatus] = React.useState<ActionStatus | null>(null);
  const action = React.useMemo(() => subscriptionAction(location.hash), [location.hash]);
  const componentIds = chosenComponentIds(scope, chosen);
  const scopeInvalid = scope === 'selected' && chosen.length === 0;

  const clearAction = React.useCallback(() => {
    navigate({ pathname: location.pathname, search: location.search, hash: '' }, { replace: true });
  }, [location.pathname, location.search, navigate]);

  const subscribe = useMutation({
    mutationFn: () =>
      statusPagesApi.subscribePublic(
        slug,
        privatePage ? 'email' : channel,
        privatePage ? '' : target,
        componentIds,
      ),
    onSuccess: () => setAccepted(true),
  });
  const addToSlack = useMutation({
    mutationFn: () => statusPagesApi.beginSlackSubscription(slug, componentIds),
    // Slack asks which channel, then sends the reader back to this page.
    onSuccess: ({ authorize_url }) => window.location.assign(authorize_url),
  });

  React.useEffect(() => {
    if (action?.kind !== 'confirm' && action?.kind !== 'unsubscribe') return;
    let cancelled = false;
    setActionStatus('confirming');
    const request = action.kind === 'confirm'
      ? statusPagesApi.confirmPublicSubscription(slug, action.token)
      : statusPagesApi.unsubscribePublic(slug, action.token);
    void request
      .then(() => {
        if (!cancelled) setActionStatus(action.kind === 'confirm' ? 'confirmed' : 'unsubscribed');
      })
      .catch(() => {
        if (!cancelled) setActionStatus('failed');
      })
      .finally(() => {
        if (!cancelled) clearAction();
      });
    return () => {
      cancelled = true;
    };
  }, [action, clearAction, slug]);

  React.useEffect(() => {
    if (action?.kind !== 'slack_connected' && action?.kind !== 'slack_failed') return;
    setActionStatus(action.kind);
    clearAction();
  }, [action, clearAction]);

  const error = subscribe.error ?? addToSlack.error;
  const targetType = channel === 'email' ? 'email' : 'url';

  return (
    <div className="flex min-w-0 shrink-0 flex-col items-end gap-2">
      <Dialog
        open={open}
        onOpenChange={(nextOpen) => {
          setOpen(nextOpen);
          if (!nextOpen) {
            setAccepted(false);
            subscribe.reset();
            addToSlack.reset();
          }
        }}
      >
        <DialogTrigger asChild>
          <Button
            type="button"
            size="lg"
            className={cn(PUBLIC_CONTROL_HEIGHT, 'bg-[var(--status-brand)] px-4 text-white hover:brightness-95')}
          >
            <BellPlus aria-hidden className="h-4 w-4" />
            {copy('public.subscription.trigger')}
          </Button>
        </DialogTrigger>
        <DialogContent
          data-theme="light"
          style={{ '--status-brand': brandColor } as React.CSSProperties}
          className="max-h-[calc(100vh-24px)] w-[min(520px,calc(100vw-24px))] gap-0 overflow-y-auto border-bd-1 bg-white p-0 text-tx-1"
        >
          <DialogHeader className="border-b border-bd-0 px-5 pb-5 pt-6 sm:px-6">
            <DialogTitle className="text-xl font-display-strong text-tx-0">
              {copy('public.subscription.title')}
            </DialogTitle>
            <DialogDescription className="mt-2 text-sm leading-6 text-tx-2">
              {copy('public.subscription.description')}
            </DialogDescription>
          </DialogHeader>

          {accepted ? (
            <div className="px-5 py-8 sm:px-6">
              <p role="status" className="text-base font-strong text-tx-0">
                {copy('public.subscription.accepted_title')}
              </p>
              <p className="mt-2 text-sm leading-6 text-tx-2">
                {copy('public.subscription.accepted_description')}
              </p>
            </div>
          ) : (
            <form
              className="space-y-5 px-5 py-6 sm:px-6"
              onSubmit={(event) => {
                event.preventDefault();
                if (!scopeInvalid && (privatePage || target.trim())) subscribe.mutate();
              }}
            >
              {!privatePage && (
                <label className="block text-sm font-strong text-tx-1">
                  <span>{copy('public.subscription.channel')}</span>
                  <Select
                    value={channel}
                    onValueChange={(value) => {
                      setChannel(value as StatusPageSubscriberChannel);
                      setTarget('');
                      subscribe.reset();
                    }}
                  >
                    <SelectTrigger className={cn(PUBLIC_CONTROL_HEIGHT, 'mt-2 w-full text-base sm:text-sm')}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent data-theme="light">
                      {(['email', 'webhook', 'slack'] as const).map((option) => (
                        <SelectItem key={option} value={option} className="min-h-10">
                          {copy(`public.subscription.${option}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </label>
              )}

              {!privatePage && channel === 'slack' && slackApp && (
                <div className="space-y-2">
                  <Button
                    type="button"
                    variant="outline"
                    disabled={scopeInvalid || addToSlack.isPending}
                    className={cn(PUBLIC_CONTROL_HEIGHT, 'w-full px-3.5')}
                    onClick={() => addToSlack.mutate()}
                  >
                    <Slack aria-hidden className="h-4 w-4" />
                    {copy('public.subscription.add_to_slack')}
                  </Button>
                  <p className="text-xs leading-5 text-tx-3">{copy('public.subscription.add_to_slack_hint')}</p>
                </div>
              )}

              {!privatePage ? (
                <label className="block text-sm font-strong text-tx-1">
                  <span>{copy(`public.subscription.${channel}_target`)}</span>
                  <Input
                    type={targetType}
                    inputMode={targetType}
                    value={target}
                    onChange={(event) => setTarget(event.target.value)}
                    required
                    autoComplete={channel === 'email' ? 'email' : 'url'}
                    placeholder={copy(`public.subscription.${channel}_placeholder`)}
                    className={cn(PUBLIC_CONTROL_HEIGHT, 'mt-2 text-base sm:text-sm')}
                  />
                  <span className="mt-2 block text-xs leading-5 text-tx-3">
                    {copy(`public.subscription.${channel}_hint`)}
                  </span>
                </label>
              ) : (
                <p className="rounded-md bg-bg-1 px-3 py-3 text-sm leading-6 text-tx-2">
                  {copy('public.subscription.private_email_hint')}
                </p>
              )}

              <ComponentChoice
                components={components}
                scope={scope}
                chosen={chosen}
                onScopeChange={setScope}
                onChosenChange={setChosen}
                copy={copy}
              />

              {error && (
                <p role="alert" className="rounded-md bg-red-dim px-3 py-2 text-sm text-red">
                  {toApiError(error).message}
                </p>
              )}

              <DialogFooter className="items-center justify-between gap-3 space-x-0 pt-1">
                {!privatePage && (
                  <Button asChild type="button" variant="outline" className={cn(PUBLIC_CONTROL_HEIGHT, 'px-3.5')}>
                    <a href={statusPagesApi.publicRssUrl(slug)} target="_blank" rel="noreferrer">
                      <Rss aria-hidden className="h-4 w-4" />
                      {copy('public.subscription.rss')}
                    </a>
                  </Button>
                )}
                <Button
                  type="submit"
                  size="lg"
                  disabled={(!privatePage && !target.trim()) || scopeInvalid || subscribe.isPending}
                  className={cn(PUBLIC_CONTROL_HEIGHT, 'bg-[var(--status-brand)] text-white hover:brightness-95')}
                >
                  {subscribe.isPending
                    ? copy('public.subscription.submitting')
                    : copy('public.subscription.submit')}
                </Button>
              </DialogFooter>
            </form>
          )}
        </DialogContent>
      </Dialog>

      {action?.kind === 'manage' && (
        <ManageSubscription
          slug={slug}
          token={action.token}
          components={components}
          brandColor={brandColor}
          copy={copy}
          onClose={clearAction}
        />
      )}

      {actionStatus && (
        <p role="status" className="max-w-xs text-right text-xs leading-5 text-tx-2">
          {copy(`public.subscription.action_${actionStatus}`)}
        </p>
      )}
    </div>
  );
}
