import { useMutation, useQuery, type UseMutationResult } from '@tanstack/react-query';
import { CircleAlert, CircleCheck, Loader2 } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useParams } from 'react-router-dom';

import * as actionsApi from '@/api/notify/actions';
import { detectBrowserLocale } from '@/i18n';
import { toApiError } from '@/lib/http';
import { ChromeButton, Pill } from '@/shell/chrome';
import { LogoMark } from '@/shell/LogoMark';
import { AuthStage } from '@/shell/signin/AuthStage';

import { formatMicros } from '../model';
import { pageLanguage, refusalOf, settledRefusal, severityTone } from './model';

const LINK_CLASS = 'mt-5 self-center font-sans text-sm font-strong text-indigo-soft hover:underline';

/**
 * Where a one-time link from a notification lands, signed in or not: the alert, what the link does,
 * and one button that does it. Nothing happens on load, since mail scanners open links before people
 * do. The page reads in the language the notification was sent in.
 */
export function NotifyActionPage() {
  const { token = '' } = useParams<{ token: string }>();
  const link = useQuery({
    queryKey: ['notify', 'action-link', token],
    queryFn: () => actionsApi.peekAction(token),
    enabled: token !== '',
    retry: false,
    refetchOnWindowFocus: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
  const act = useMutation({ mutationFn: () => actionsApi.takeAction(token) });
  const lng = pageLanguage(link.data?.language, detectBrowserLocale());
  const { t } = useTranslation(['notify', 'alerts'], { lng });

  let body: React.ReactNode;

  if (token === '' || link.isError) {
    const error = link.isError ? toApiError(link.error) : null;
    const gone = error === null || error.status === 404;

    body = (
      <>
        <CircleAlert className="h-6 w-6 self-center text-tx-3" aria-hidden="true" />
        <h1 className="mt-3 text-center font-sans text-base font-display-strong text-tx-0">
          {t(gone ? 'action_link.unavailable_title' : 'action_link.load_failed_title')}
        </h1>
        <p className="mt-2 text-center font-sans text-sm leading-relaxed text-tx-2">
          {gone ? t('action_link.unavailable_hint') : error.message}
        </p>
        <Link to="/alerts/incidents" className={LINK_CLASS}>
          {t('action_link.open_alerts')}
        </Link>
      </>
    );
  } else if (link.isPending) {
    body = (
      <p role="status" className="flex items-center justify-center gap-2 font-sans text-sm text-tx-2">
        <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
        {t('action_link.loading')}
      </p>
    );
  } else {
    const { alert, action } = link.data;
    // Once the link has acted, the alert stands where it put it.
    const status = act.isSuccess ? act.data.status : alert.status;

    body = (
      <>
        <div className="flex flex-wrap items-center gap-2">
          <Pill tone={severityTone(alert.severity)}>
            {t(`alerts:severity.${alert.severity}`, { defaultValue: alert.severity })}
          </Pill>
          <span className="font-sans text-xs text-tx-2">
            {t(`alerts:history.status.${status}`, { defaultValue: status })}
          </span>
        </div>
        <h1 className="mt-3 break-words font-sans text-base font-display-strong text-tx-0">{alert.title}</h1>
        <p className="mt-1 font-sans text-xs text-tx-3">
          {t('action_link.started', { at: formatMicros(alert.startedAt, lng) })}
        </p>
        <div className="mt-5">
          <ActionOutcome lng={lng} link={link.data} act={act} action={action} />
        </div>
        <Link to={`/alerts/incidents/${encodeURIComponent(alert.id)}`} className={LINK_CLASS}>
          {t('action_link.open_alert')}
        </Link>
      </>
    );
  }

  return (
    <AuthStage>
      <LogoMark size={30} />
      <section
        data-notify-action
        className="ms-auth-card mt-6 flex w-full min-w-0 flex-col rounded-2xl border border-bd-0 bg-bg-1 p-6 shadow-login sm:p-8"
      >
        {body}
      </section>
    </AuthStage>
  );
}

/** What the link did or would do: the button while it can act, then what came of pressing it. */
function ActionOutcome({
  lng,
  link,
  act,
  action,
}: {
  lng: string;
  link: actionsApi.NotifyActionLink;
  act: UseMutationResult<actionsApi.NotifyActionResult, Error, void>;
  action: actionsApi.NotifyActionKind;
}) {
  const { t } = useTranslation('notify', { lng });

  if (act.isSuccess) {
    return (
      <Note tone="done">{t(`action_link.${act.data.status === 'resolved' ? 'resolved' : 'acknowledged'}`)}</Note>
    );
  }

  const error = act.isError ? toApiError(act.error) : null;
  const refusal = error ? refusalOf(error) : settledRefusal(link);

  if (refusal) {
    return (
      <Note tone="refused">
        {refusal === 'used' && link.usedAt != null
          ? t('action_link.used_at', { at: formatMicros(link.usedAt, lng) })
          : t(`action_link.${refusal}`)}
      </Note>
    );
  }

  return (
    <>
      <p className="font-sans text-sm leading-relaxed text-tx-1">
        {t(`action_link.explain_${action}`, { until: formatMicros(link.expiresAt, lng) })}
      </p>
      {error && (
        <p role="alert" className="mt-3 font-sans text-sm text-[var(--red-ink)]">
          {t('action_link.failed', { error: error.message })}
        </p>
      )}
      <ChromeButton
        variant="primary"
        className="mt-4 h-11 w-full justify-center"
        disabled={act.isPending}
        onClick={() => act.mutate()}
      >
        {act.isPending
          ? t(action === 'resolve' ? 'action_link.resolving' : 'action_link.acknowledging')
          : t(`action_link.${action}`)}
      </ChromeButton>
    </>
  );
}

/** A line saying how it ended: done, or why the link will not act. */
function Note({ tone, children }: { tone: 'done' | 'refused'; children: React.ReactNode }) {
  const Icon = tone === 'done' ? CircleCheck : CircleAlert;

  return (
    <p
      role="status"
      data-action-outcome={tone}
      className="flex items-start gap-2 rounded-xl bg-bg-2 px-3.5 py-3 font-sans text-sm leading-relaxed text-tx-1"
    >
      <Icon
        className={`mt-0.5 h-4 w-4 shrink-0 ${tone === 'done' ? 'text-[var(--green-ink)]' : 'text-[var(--orange-ink)]'}`}
        aria-hidden="true"
      />
      <span className="min-w-0">{children}</span>
    </p>
  );
}
