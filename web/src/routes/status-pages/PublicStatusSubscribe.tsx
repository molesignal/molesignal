import { useMutation } from '@tanstack/react-query';
import { BellPlus, Rss } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type {
  StatusPageLanguage,
  StatusPageSubscriberChannel,
} from '@/api/statusPages';
import { toApiError } from '@/lib/http';
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

export function PublicStatusSubscribe({
  slug,
  language,
  privatePage = false,
}: {
  slug: string;
  language: StatusPageLanguage;
  privatePage?: boolean;
}) {
  const { i18n } = useTranslation('status-pages');
  const copy = i18n.getFixedT(language, 'status-pages');
  const location = useLocation();
  const navigate = useNavigate();
  const [open, setOpen] = React.useState(false);
  const [channel, setChannel] = React.useState<StatusPageSubscriberChannel>('email');
  const [target, setTarget] = React.useState('');
  const [accepted, setAccepted] = React.useState(false);
  const [actionStatus, setActionStatus] = React.useState<
    'confirming' | 'confirmed' | 'unsubscribed' | 'failed' | null
  >(null);

  const subscribe = useMutation({
    mutationFn: () => statusPagesApi.subscribePublic(slug, privatePage ? 'email' : channel, privatePage ? '' : target),
    onSuccess: () => setAccepted(true),
  });

  const actionParams = React.useMemo(
    () => new URLSearchParams(location.hash.replace(/^#/, '')),
    [location.hash],
  );
  const action = actionParams.get('subscription_action');
  const token = actionParams.get('subscription_token');
  React.useEffect(() => {
    if (!token || (action !== 'confirm' && action !== 'unsubscribe')) return;
    let cancelled = false;
    setActionStatus('confirming');
    const request = action === 'confirm'
      ? statusPagesApi.confirmPublicSubscription(slug, token)
      : statusPagesApi.unsubscribePublic(slug, token);
    void request
      .then(() => {
        if (!cancelled) setActionStatus(action === 'confirm' ? 'confirmed' : 'unsubscribed');
      })
      .catch(() => {
        if (!cancelled) setActionStatus('failed');
      })
      .finally(() => {
        if (cancelled) return;
        navigate(
          { pathname: location.pathname, search: location.search, hash: '' },
          { replace: true },
        );
      });
    return () => {
      cancelled = true;
    };
  }, [action, location.pathname, location.search, navigate, slug, token]);

  const error = subscribe.error ? toApiError(subscribe.error).message : null;
  const targetLabel = channel === 'email'
    ? copy('public.subscription.email')
    : copy('public.subscription.webhook');

  return (
    <div className="flex min-w-0 shrink-0 flex-col items-end gap-2">
      <Dialog
        open={open}
        onOpenChange={(nextOpen) => {
          setOpen(nextOpen);
          if (!nextOpen) {
            setAccepted(false);
            subscribe.reset();
          }
        }}
      >
        <DialogTrigger asChild>
          <Button
            type="button"
            size="lg"
            className="h-11 bg-[var(--status-brand)] px-4 text-white hover:brightness-95"
          >
            <BellPlus aria-hidden className="h-4 w-4" />
            {copy('public.subscription.trigger')}
          </Button>
        </DialogTrigger>
        <DialogContent
          data-theme="light"
          className="w-[min(520px,calc(100vw-24px))] gap-0 border-bd-1 bg-white p-0 text-tx-1"
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
                if (privatePage || target.trim()) subscribe.mutate();
              }}
            >
              {!privatePage && <label className="block text-sm font-strong text-tx-1">
                <span>{copy('public.subscription.channel')}</span>
                <Select
                  value={channel}
                  onValueChange={(value) => {
                    setChannel(value as StatusPageSubscriberChannel);
                    setTarget('');
                    subscribe.reset();
                  }}
                >
                  <SelectTrigger className="mt-2 h-11 w-full bg-white text-base sm:text-sm">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent data-theme="light">
                    <SelectItem value="email" className="min-h-10">
                      {copy('public.subscription.email')}
                    </SelectItem>
                    <SelectItem value="webhook" className="min-h-10">
                      {copy('public.subscription.webhook')}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </label>}

              {!privatePage ? <label className="block text-sm font-strong text-tx-1">
                <span>{targetLabel}</span>
                <Input
                  type={channel === 'email' ? 'email' : 'url'}
                  inputMode={channel === 'email' ? 'email' : 'url'}
                  value={target}
                  onChange={(event) => setTarget(event.target.value)}
                  required
                  autoComplete={channel === 'email' ? 'email' : 'url'}
                  placeholder={copy(`public.subscription.${channel}_placeholder`)}
                  className="mt-2 h-11 bg-white text-base sm:text-sm"
                />
                <span className="mt-2 block text-xs font-normal leading-5 text-tx-3">
                  {copy(`public.subscription.${channel}_hint`)}
                </span>
              </label> : (
                <p className="rounded-md bg-bg-1 px-3 py-3 text-sm leading-6 text-tx-2">
                  {copy('public.subscription.private_email_hint')}
                </p>
              )}

              {error && (
                <p role="alert" className="rounded-md bg-red-dim px-3 py-2 text-sm text-red">
                  {error}
                </p>
              )}

              <DialogFooter className="items-center justify-between gap-3 space-x-0 pt-1">
                {!privatePage && (
                  <Button asChild type="button" variant="ghost" className="h-11 px-3 text-tx-2">
                    <a href={statusPagesApi.publicRssUrl(slug)} target="_blank" rel="noreferrer">
                      <Rss aria-hidden className="h-4 w-4" />
                      {copy('public.subscription.rss')}
                    </a>
                  </Button>
                )}
                <Button
                  type="submit"
                  size="lg"
                  disabled={(!privatePage && !target.trim()) || subscribe.isPending}
                  className="h-11 bg-[var(--status-brand)] text-white hover:brightness-95"
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

      {actionStatus && (
        <p role="status" className="max-w-xs text-right text-xs leading-5 text-tx-2">
          {copy(`public.subscription.action_${actionStatus}`)}
        </p>
      )}
    </div>
  );
}
