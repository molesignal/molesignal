import { useTranslation } from 'react-i18next';
import { Link, useParams, useSearchParams } from 'react-router-dom';

import { type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { SignalReference } from '@/shell/SignalReference';
import { useTrace } from '@/viz/trace/loader';
import { TraceFlame } from '@/viz/trace/TraceFlame';

export function TraceDetail() {
  const { t } = useTranslation('traces');
  const { t: tProfiles } = useTranslation('profiles');
  const { id } = useParams<{ id: string }>();
  const [searchParams] = useSearchParams();
  const spanId = searchParams.get('spanId') ?? undefined;
  const traceQuery = useTrace(id);
  const sessionId = relatedSessionId(traceQuery.data?.spans ?? []);
  const profilesHref = id
    ? `/profiles?trace_id=${encodeURIComponent(id)}${spanId ? `&span_id=${encodeURIComponent(spanId)}` : ''}`
    : '';
  const state: ProductStateProps | null = id
    ? null
    : {
        variant: 'empty',
        title: t('detail.missing_title'),
        description: t('detail.missing_description'),
      };

  return (
    <DetailPage
      title={t('detail.title')}
      toolbar={
        id ? (
          <div className="flex items-center gap-2">
            {sessionId && (
              <Link to={`/rum/sessions/view/${encodeURIComponent(sessionId)}`}>
                <ChromeButton>{t('detail.view_user_session')}</ChromeButton>
              </Link>
            )}
            <Link to={profilesHref}>
              <ChromeButton>{tProfiles('detail.view_span_flamegraph')}</ChromeButton>
            </Link>
            <Link to={`/logs?traceId=${encodeURIComponent(id)}`}>
              <ChromeButton variant="primary">{t('detail.search_logs')}</ChromeButton>
            </Link>
          </div>
        ) : null
      }
      metadata={[
        { label: t('detail.back'), value: <Link to="/traces" className="text-indigo-soft hover:underline">{t('detail.back')}</Link> },
        // Phase 6 M2: trace_id is a canonical cross-signal handle.
        // Wrapping it in SignalReference lets users jump to related
        // logs / metrics without leaving the trace view.
        ...(id ? [{ label: t('detail.trace_id'), value: <SignalReference type="trace_id" value={id}>{id}</SignalReference> }] : []),
      ]}
      bodyClassName="p-4"
      state={state}
    >
      {id && <TraceFlame traceId={id} {...(spanId ? { initialSpanId: spanId } : {})} />}
    </DetailPage>
  );
}

function relatedSessionId(
  spans: Array<{
    attributes: Record<string, unknown>;
    resource: { attributes: Record<string, unknown> };
  }>,
): string | undefined {
  const keys = [
    'session.id',
    'session_id',
    'rum.session_id',
    'browser.session.id',
  ] as const;
  for (const span of spans) {
    for (const attributes of [span.attributes, span.resource.attributes]) {
      for (const key of keys) {
        const value = attributes[key];
        if (typeof value === 'string' && value.trim()) return value;
      }
    }
  }
  return undefined;
}
