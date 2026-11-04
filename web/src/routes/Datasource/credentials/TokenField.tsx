import { Eye, EyeOff } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { TokenScopeLabel } from '@/routes/iam/ApiTokens/scope';
import { IconButton } from '@/shell/chrome';
import { CopyField } from '@/shell/CopyField';
import { DisabledControl } from '@/shell/DisabledControl';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { maskToken } from '../datasourceModel';
import type { IntakeContext } from '../intakeContext';

/**
 * The token a source authenticates with: masked until asked for, with what it
 * may do, where and until when on the line under it.
 */
export function TokenField({ context }: { context: IntakeContext }) {
  const { t } = useTranslation('onboarding');
  // An application's RUM token is managed with the organization's tokens; the
  // default intake token is the signed-in user's own.
  const tokenManageAccess = useActionAccess({
    permission: context.isRum ? 'api_tokens.issue' : 'personal_tokens.manage',
  });
  const manageTokensPath = context.isRum ? '/iam/api-tokens' : '/account/settings/api-tokens';
  const [revealed, setRevealed] = React.useState(false);
  const revealLabel = revealed
    ? t('datasource_page.hide_token')
    : t('datasource_page.show_token');

  // An app-bound token cannot be issued before the application is chosen.
  if (context.isRum && !context.applicationValid) {
    return (
      <p className="m-0 flex min-h-9 items-center font-sans text-sm leading-relaxed text-tx-3">
        {t('datasource_page.rum_application_required')}
      </p>
    );
  }

  if (context.tokenError) {
    return (
      <p
        role="alert"
        title={context.tokenError}
        className="m-0 flex min-h-9 items-center rounded-md bg-red-dim px-3 py-2 font-sans text-sm text-red-soft"
      >
        {t('datasource_page.token_load_failed')}
      </p>
    );
  }

  const unavailable = context.tokenLoading || !context.token;

  return (
    <>
      <CopyField
        value={context.token}
        copyLabel={t('datasource.copy_token')}
        disabled={unavailable}
        actions={
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex shrink-0">
                <IconButton
                  aria-label={revealLabel}
                  aria-pressed={revealed}
                  disabled={unavailable}
                  onClick={() => setRevealed((current) => !current)}
                >
                  {revealed ? (
                    <EyeOff aria-hidden className="h-3.5 w-3.5" />
                  ) : (
                    <Eye aria-hidden className="h-3.5 w-3.5" />
                  )}
                </IconButton>
              </span>
            </TooltipTrigger>
            <TooltipContent side="top">{revealLabel}</TooltipContent>
          </Tooltip>
        }
      >
        {context.tokenLoading
          ? t('datasource.token_loading')
          : !context.token
            ? '—'
            : revealed
              ? context.token
              : maskToken(context.token)}
      </CopyField>
      <div className="mt-[6px] flex min-w-0 flex-wrap items-center gap-x-[14px] gap-y-1 font-sans text-xs text-tx-3">
        <TokenFact
          label={t('datasource_page.token_permission')}
          value={
            context.tokenScope ? (
              <TokenScopeLabel token={context.tokenScope} />
            ) : (
              '—'
            )
          }
        />
        <TokenFact label={t('datasource_page.token_workspace')} value={context.orgLabel} />
        <TokenFact
          label={t('datasource_page.token_expiry')}
          value={t('datasource_page.token_never_expires')}
        />
        <DisabledControl disabled={tokenManageAccess.disabled} reason={tokenManageAccess.reason}>
          {tokenManageAccess.disabled ? (
            <span aria-disabled className="ml-auto cursor-not-allowed font-strong text-tx-3">
              {t('datasource_page.manage_tokens')}
            </span>
          ) : (
            <Link
              to={manageTokensPath}
              className="ml-auto rounded font-strong text-indigo-soft outline-none hover:underline focus-visible:underline"
            >
              {t('datasource_page.manage_tokens')}
            </Link>
          )}
        </DisabledControl>
      </div>
    </>
  );
}

function TokenFact({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <span className="min-w-0 truncate">
      {label} <span className="font-strong text-tx-2">{value}</span>
    </span>
  );
}
