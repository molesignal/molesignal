import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as ssoApi from '@/api/sso';
import { writeClipboardText } from '@/lib/clipboard';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';

import { failureCodeOf, knownNotes, rolesChange } from './model';

type Tone = 'green' | 'red' | 'blue' | 'orange';

const TINT: Record<Tone, string> = {
  green: 'bg-[var(--green-tint)] text-[var(--green-ink)]',
  red: 'bg-red-dim text-[var(--red-ink)]',
  blue: 'bg-[var(--blue-tint)] text-[var(--blue-ink)]',
  orange: 'bg-[var(--orange-tint)] text-[var(--orange-ink)]',
};

function Tinted({
  tone,
  title,
  children,
  testId,
}: {
  tone: Tone;
  title: React.ReactNode;
  children?: React.ReactNode;
  testId?: string;
}) {
  return (
    <div data-testid={testId} className={cn('rounded-md px-3 py-2.5 font-sans', TINT[tone])}>
      <div className="text-sm font-display">{title}</div>
      {children && <div className="mt-1 text-sm leading-relaxed">{children}</div>}
    </div>
  );
}

/** The words of the server or the identity provider, kept to pass on to whoever runs it. */
function FailureDetail({ detail }: { detail: string }) {
  const { t } = useTranslation('settings-admin');
  const [copied, setCopied] = React.useState(false);

  const copy = async () => {
    try {
      await writeClipboardText(detail);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // The clipboard is blocked: the text is on screen to select by hand.
    }
  };

  return (
    <div className="mt-3 flex min-w-0 items-start gap-2 rounded-md bg-[var(--page-canvas)] py-1 pl-3 pr-1">
      <code
        data-testid="sso-test-detail"
        className="min-w-0 flex-1 whitespace-pre-wrap py-1 font-mono text-xs leading-relaxed text-tx-1 [overflow-wrap:anywhere]"
      >
        {detail}
      </code>
      <CopyIconButton
        onClick={() => void copy()}
        label={t('sso_providers.test.result.copy_detail')}
        copied={copied}
        copiedLabel={t('sso_providers.test.result.copied_detail')}
      />
    </div>
  );
}

/** Whether the test went through, first and in one place. */
export function ResultVerdict({ test }: { test: ssoApi.SsoSigninTest }) {
  const { t } = useTranslation('settings-admin');

  const stale = !test.current && (test.status === 'passed' || test.status === 'failed') && (
    <Tinted
      tone="orange"
      title={t('sso_providers.test.result.stale_title')}
      testId="sso-test-stale"
    >
      {t('sso_providers.test.result.stale_body')}
    </Tinted>
  );

  if (test.status === 'pending') {
    return (
      <Tinted tone="blue" title={t('sso_providers.test.result.pending_title')} testId="sso-test-verdict">
        {t('sso_providers.test.result.pending_body')}
      </Tinted>
    );
  }

  if (test.status === 'expired') {
    return (
      <Tinted tone="orange" title={t('sso_providers.test.result.expired_title')} testId="sso-test-verdict">
        {t('sso_providers.test.result.expired_body')}
      </Tinted>
    );
  }

  if (test.status === 'passed') {
    return (
      <>
        {stale}
        <Tinted tone="green" title={t('sso_providers.test.result.passed_title')} testId="sso-test-verdict">
          {t('sso_providers.test.result.passed_body')}
        </Tinted>
      </>
    );
  }

  const code = failureCodeOf(test.failure?.code);

  return (
    <>
      {stale}
      <div>
        <Tinted
          tone="red"
          title={t(`sso_providers.test.failures.${code}.title`)}
          testId="sso-test-verdict"
        >
          {t(`sso_providers.test.failures.${code}.advice`)}
        </Tinted>
        {test.failure?.detail && <FailureDetail detail={test.failure.detail} />}
      </div>
    </>
  );
}

/** What a passing test still wants looked at. */
export function ResultNotes({ test }: { test: ssoApi.SsoSigninTest }) {
  const { t } = useTranslation('settings-admin');
  const notes = knownNotes(test.notes);

  if (notes.length === 0) return null;

  return (
    <Tinted tone="orange" title={t('sso_providers.test.result.notes_title')} testId="sso-test-notes">
      <ul className="flex flex-col gap-1">
        {notes.map((note) => (
          <li key={note}>{t(`sso_providers.test.notes.${note}`)}</li>
        ))}
      </ul>
    </Tinted>
  );
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[96px_minmax(0,1fr)] items-baseline gap-3 py-1.5 sm:grid-cols-[120px_minmax(0,1fr)]">
      <dt className="text-tx-2">{label}</dt>
      <dd className="min-w-0 text-tx-0 [overflow-wrap:anywhere]">{children}</dd>
    </div>
  );
}

function FindingSection({
  title,
  testId,
  children,
}: {
  title: string;
  testId: string;
  children: React.ReactNode;
}) {
  return (
    <section data-testid={testId} className="min-w-0">
      <h3 className="font-sans text-sm font-display text-tx-0">{title}</h3>
      <dl className="mt-1.5 font-sans text-sm">{children}</dl>
    </section>
  );
}

function Groups({ identity, matched }: { identity: ssoApi.SsoTestedIdentity; matched: readonly string[] }) {
  const { t } = useTranslation('settings-admin');

  if (identity.groups.length === 0) {
    return <span className="text-tx-3">{t('sso_providers.test.result.no_groups')}</span>;
  }

  const hidden = identity.groupsTotal - identity.groups.length;

  return (
    <ul className="flex flex-wrap gap-x-3 gap-y-0.5" data-testid="sso-test-groups">
      {identity.groups.map((group) => (
        <li key={group} className={cn('min-w-0', matched.includes(group) && 'font-strong')}>
          {group}
        </li>
      ))}
      {hidden > 0 && (
        <li className="text-tx-3">{t('sso_providers.test.result.more_groups', { count: hidden })}</li>
      )}
    </ul>
  );
}

function Roles({ mapping }: { mapping: ssoApi.SsoTestedMapping }) {
  const { t } = useTranslation('settings-admin');
  const names = (roles: readonly ssoApi.SsoTestedRole[]) =>
    roles.map((role) => role.name).join(t('sso_providers.access.list_separator')) ||
    t('sso_providers.test.result.no_roles');
  const source = t(`sso_providers.test.result.role_source.${mapping.roleSource}`, {
    groups: mapping.matchedGroups.join(t('sso_providers.access.list_separator')),
  });

  return (
    <>
      {rolesChange(mapping) ? (
        <span data-testid="sso-test-roles">
          <span className="text-tx-2">{names(mapping.currentRoles)}</span>
          {' → '}
          <span className="font-strong">{names(mapping.roles)}</span>
        </span>
      ) : (
        <span data-testid="sso-test-roles">{names(mapping.roles)}</span>
      )}
      <span className="mt-0.5 block text-xs text-tx-3">{source}</span>
    </>
  );
}

function accountSentenceKey(mapping: ssoApi.SsoTestedMapping): string {
  const who = mapping.isTester ? 'you' : 'other';
  if (mapping.account === 'create') return 'sso_providers.test.result.account.create';
  return `sso_providers.test.result.account.${mapping.account}_${who}`;
}

/** What the identity provider returned, and what a real sign-in would do with it. */
export function Findings({ test }: { test: ssoApi.SsoSigninTest }) {
  const { t } = useTranslation('settings-admin');
  const { identity, mapping } = test;

  return (
    <>
      {identity && (
        <FindingSection title={t('sso_providers.test.result.identity_title')} testId="sso-test-identity">
          <Fact label={t('sso_providers.test.result.email')}>
            {identity.email}
            {identity.emailVerified === false && (
              <span className="ml-2 text-xs text-[var(--orange-ink)]">
                {t('sso_providers.test.result.email_unverified')}
              </span>
            )}
          </Fact>
          <Fact label={t('sso_providers.test.result.name')}>{identity.displayName || '–'}</Fact>
          <Fact label={t('sso_providers.test.result.subject')}>
            <code className="font-mono text-xs">{identity.subject}</code>
          </Fact>
          <Fact label={t('sso_providers.test.result.groups')}>
            <Groups identity={identity} matched={mapping?.matchedGroups ?? []} />
          </Fact>
        </FindingSection>
      )}
      {mapping && (
        <FindingSection title={t('sso_providers.test.result.mapping_title')} testId="sso-test-mapping">
          <Fact label={t('sso_providers.test.result.account_label')}>
            {t(accountSentenceKey(mapping), { email: mapping.accountEmail })}
            {!mapping.member && mapping.account !== 'create' && (
              <span className="mt-0.5 block text-xs text-tx-3">
                {t('sso_providers.test.result.account.joins')}
              </span>
            )}
          </Fact>
          <Fact label={t('sso_providers.test.result.roles_label')}>
            <Roles mapping={mapping} />
          </Fact>
        </FindingSection>
      )}
    </>
  );
}
