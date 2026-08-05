import { Eye, EyeOff } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { DisabledControl } from '@/shell/DisabledControl';
import { cn } from '@/shell/lib/cn';

import { maskToken } from '../datasourceModel';
import type { IngestContext } from '../ingestContext';

export function TokenPanel({ context }: { context: IngestContext }) {
  const { t, i18n } = useTranslation('onboarding');
  const navigate = useNavigate();
  const tokenManageAccess = useActionAccess({ permission: 'api_tokens.manage' });
  const [revealed, setRevealed] = React.useState(false);
  const [copied, setCopied] = React.useState(false);
  const copy = async () => {
    if (!context.token) return;
    try {
      await navigator.clipboard.writeText(context.token);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };
  const expiry = context.tokenExpiresAtMicros
    ? new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' }).format(
        new Date(context.tokenExpiresAtMicros / 1000),
      )
    : t('datasource_page.token_never_expires');
  const blocked = context.isRum && !context.applicationValid;

  return (
    <div className="min-w-0 rounded-md border border-bd-0 bg-bg-1 p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className={uiLabelClass}>
          {context.isRum
            ? t('datasource.rum_client_token')
            : t('datasource.ingestion_token')}
        </span>
        <DisabledControl disabled={tokenManageAccess.disabled} reason={tokenManageAccess.reason}>
          <button
            type="button"
            disabled={tokenManageAccess.disabled}
            aria-disabled={tokenManageAccess.disabled || undefined}
            onClick={() => navigate('/iam/service-accounts')}
            className="font-sans text-xs font-strong text-indigo-soft enabled:hover:underline disabled:cursor-not-allowed disabled:text-tx-3"
          >
            {t('datasource_page.manage_tokens')}
          </button>
        </DisabledControl>
      </div>
      {blocked ? (
        <div className="rounded border border-yellow/30 bg-yellow-dim px-2.5 py-2 font-sans text-xs text-yellow-soft">
          {t('datasource_page.rum_application_required')}
        </div>
      ) : context.tokenError ? (
        <div
          className="rounded border border-red/30 bg-red-dim px-2.5 py-2 font-sans text-xs text-red-soft"
          title={context.tokenError}
        >
          {t('datasource_page.token_load_failed')}
        </div>
      ) : (
        <div className="flex min-w-0 items-center gap-1.5">
          <code className="min-w-0 flex-1 truncate rounded border border-bd-0 bg-bg-2 px-2.5 py-2 font-mono text-xs text-tx-1">
            {context.tokenLoading
              ? t('datasource.token_loading')
              : revealed
                ? context.token
                : maskToken(context.token)}
          </code>
          <ChromeButton
            size="sm"
            onClick={() => setRevealed((current) => !current)}
            disabled={context.tokenLoading || !context.token}
            aria-label={
              revealed ? t('datasource_page.hide_token') : t('datasource_page.show_token')
            }
          >
            {revealed ? <EyeOff className="h-3 w-3" /> : <Eye className="h-3 w-3" />}
          </ChromeButton>
          <CopyIconButton
            onClick={copy}
            disabled={context.tokenLoading || !context.token}
            label={t('datasource.copy_token')}
            copied={copied}
            copiedLabel={t('datasource_page.copied')}
          />
        </div>
      )}
      <dl
        className={cn(
          'mt-2 grid gap-2 border-t border-bd-0 pt-2 font-sans text-xs',
          context.isRum ? 'grid-cols-2 sm:grid-cols-4' : 'grid-cols-3',
        )}
      >
        <TokenMeta label={t('datasource_page.token_permission')} value={context.tokenRole} />
        {context.isRum && (
          <TokenMeta
            label={t('datasource_page.rum_application_id')}
            value={context.applicationId || '—'}
          />
        )}
        <TokenMeta label={t('datasource_page.token_workspace')} value={context.orgLabel} />
        <TokenMeta label={t('datasource_page.token_expiry')} value={expiry} />
      </dl>
    </div>
  );
}

function TokenMeta({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-tx-3">{label}</dt>
      <dd className="mt-0.5 truncate font-strong text-tx-1">{value}</dd>
    </div>
  );
}
