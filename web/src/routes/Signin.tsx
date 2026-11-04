import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import * as auth from '@/api/auth';
import * as instanceApi from '@/api/instance';
import * as meApi from '@/api/me';
import * as ssoApi from '@/api/sso';
import { resolveDefaultHomeRoute } from '@/lib/homeRoute';
import { toApiError } from '@/lib/http';
import { passkeysSupported } from '@/lib/webauthn';
import { EnrollPanel } from '@/routes/signin/EnrollPanel';
import {
  keepLastSignin,
  keepSsoTrip,
  type LastSignin,
  readLastSignin,
  takeSsoTrip,
} from '@/shell/signin/lastSignin';
import { signinLimitWait } from '@/shell/signin/signinLimit';
import { SigninShell, type SigninView } from '@/shell/signin/SigninShell';
import { linkMethods, type SsoLinkMethod } from '@/shell/signin/SsoLinkPanel';
import { usePasskeySignin } from '@/shell/signin/usePasskeySignin';
import { usePasswordRecovery } from '@/shell/signin/usePasswordRecovery';
import { useSecondStep } from '@/shell/signin/useSecondStep';
import {
  type ChannelRow,
  channelRows,
  rememberedEmail,
  useChannelsKnown,
  useSigninSteps,
} from '@/shell/signin/useSigninSteps';
import { toast } from '@/shell/ui/sonner';
import { type AuthContextInput, useAuthStore } from '@/stores/auth';

interface SsoSigninOptions {
  remember: boolean;
  next?: string;
  /** How this sign-in came in: kept for the next visit once it succeeds. */
  last?: LastSignin;
}

/** A first single sign-on into an existing account, waiting for its owner to confirm. */
interface PendingLink {
  info: ssoApi.SsoLinkRequired;
  options: SsoSigninOptions;
}

/** A sign-in past its first step, waiting for a second factor to be set up. */
interface PendingEnroll {
  required: auth.MfaEnrollRequired;
  options: SsoSigninOptions;
}

export function Signin() {
  const { t, i18n } = useTranslation(['errors', 'shell']);
  const nav = useNavigate();
  const [params] = useSearchParams();
  const requestedNext = params.get('next');
  /** The single-use code a redirect sign-in (OIDC, OAuth 2.0, SAML) returns with. */
  const ssoCode = params.get('sso');
  const resetTokenFromUrl =
    params.get('resetToken') ??
    new URLSearchParams(window.location.hash.replace(/^#/, '')).get('resetToken') ??
    '';
  const [resetToken] = React.useState(resetTokenFromUrl.trim());
  const setSession = useAuthStore((s) => s.setSession);

  const [last] = React.useState(readLastSignin);
  const [email, setEmail] = React.useState(() => rememberedEmail(last));
  const [password, setPassword] = React.useState('');
  const [confirmPassword, setConfirmPassword] = React.useState('');
  const [remember, setRemember] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [view, setView] = React.useState<SigninView>(
    resetToken ? 'reset-password' : 'signin',
  );
  /** Why a redirect sign-in that came back here did not sign the user in. */
  const [ssoError, setSsoError] = React.useState<string | null>(null);
  const [link, setLink] = React.useState<PendingLink | null>(null);
  const [linkMethod, setLinkMethod] = React.useState<SsoLinkMethod>('password');
  const [linkCode, setLinkCode] = React.useState('');
  const [linkCodeSentAt, setLinkCodeSentAt] = React.useState<number | null>(null);
  const [linkSending, setLinkSending] = React.useState(false);
  const [enroll, setEnroll] = React.useState<PendingEnroll | null>(null);
  const enrollSubmit = React.useRef<(() => void) | null>(null);

  React.useEffect(() => {
    if (!resetToken) return;
    const cleanUrl = new URL(window.location.href);
    cleanUrl.searchParams.delete('resetToken');
    cleanUrl.hash = '';
    window.history.replaceState(
      window.history.state,
      '',
      `${cleanUrl.pathname}${cleanUrl.search}`,
    );
  }, [resetToken]);

  // The SSO providers list informs which buttons we render. We intentionally
  // do not gate the password form behind it — if the call fails (community
  // edition, or backend not yet wired) the password form keeps working.
  const ssoQuery = useQuery({
    queryKey: ['sso', 'providers', 'signin'],
    queryFn: () => ssoApi.listPublic(),
    retry: false,
    staleTime: 60_000,
  });

  // 公开实例信息：signup_enabled 决定是否显示「注册」入口。
  const instanceQuery = useQuery({
    queryKey: ['instance'],
    queryFn: () => instanceApi.get(),
    retry: false,
    staleTime: 60_000,
  });

  const passkeyAvailable = (instanceQuery.data?.passkeySignin ?? false) && passkeysSupported();
  const rows = channelRows(ssoQuery.data ?? [], passkeyAvailable, last);
  const channelsKnown = useChannelsKnown(ssoQuery.isLoading || instanceQuery.isLoading);
  const steps = useSigninSteps(rows, channelsKnown);

  const completeSignin = async (
    res: auth.SigninResponse,
    options: { remember?: boolean; next?: string; last?: LastSignin } = {},
  ) => {
    if (options.last) keepLastSignin(options.last, res.email);
    const nextCtx: AuthContextInput = {
      userId: res.userId,
      orgId: res.orgId,
      displayRole: res.displayRole,
      roles: res.roles,
      scope: res.scope,
    };
    if (res.email) nextCtx.email = res.email;
    if (res.displayName) nextCtx.displayName = res.displayName;
    if (res.orgName) nextCtx.orgName = res.orgName;
    setSession(nextCtx, options.remember ?? remember);
    const requested = options.next ?? requestedNext;
    let destination = requested ?? '/';
    if (!requested) {
      try {
        const preferences = await meApi.preferences();
        destination = resolveDefaultHomeRoute(
          preferences.defaultHomeRoute,
          res.userId,
          res.orgId,
        );
      } catch {
        destination = '/home';
      }
    }
    nav(destination, { replace: true });
  };

  const openLink = (info: ssoApi.SsoLinkRequired, options: SsoSigninOptions) => {
    setPassword('');
    setLinkCode('');
    setLinkCodeSentAt(null);
    setLinkMethod(linkMethods(info)[0] ?? 'password');
    setLink({ info, options });
    setView('sso-link');
  };

  const secondStep = useSecondStep<SsoSigninOptions>({
    active: view === 'mfa',
    busy,
    setBusy,
    onSignedIn: completeSignin,
    onExpired: () => setView('signin'),
  });

  const openMfa = (required: auth.MfaRequired, options: SsoSigninOptions) => {
    setPassword('');
    secondStep.open(required, options);
    setView('mfa');
  };

  /**
   * A sign-in ends in a session, in a link the account's owner confirms first, or
   * in the second step of an account with multi-factor authentication.
   */
  const settle = async (res: ssoApi.SsoSignInResult, options: SsoSigninOptions) => {
    if (res.mfa) {
      openMfa(res.mfa, options);
      return;
    }
    if (res.enroll) {
      setPassword('');
      setEnroll({ required: res.enroll, options });
      setView('mfa-enroll');
      return;
    }
    if (res.link) {
      openLink(res.link, options);
      return;
    }
    if (res.login) await completeSignin(res.login, options);
  };

  const runCredentialSignin = async (run: () => Promise<void>, limitedHint?: string) => {
    setSsoError(null);
    setBusy(true);
    try {
      await run();
    } catch (err) {
      const e = toApiError(err);
      // A locked second step answers the password with 429.
      const locked = e.status === 429 && e.message.includes('wrong verification codes');
      const ssoOnly = e.status === 403 && e.message.includes('only allows single sign-on');
      const wait = signinLimitWait(e, t);
      toast.error(t('errors:sign_in_failed'), {
        description: wait
          ? limitedHint
            ? t('shell:signin.limited_with_hint', { wait, hint: limitedHint })
            : wait
          : locked
            ? t('shell:signin.mfa.locked')
            : ssoOnly
              ? t('shell:signin.sso_only')
              : e.message,
      });
      if (ssoOnly) steps.restart();
    } finally {
      setBusy(false);
    }
  };

  const beginSso = (provider: ssoApi.PublicSsoProvider) => {
    if (provider.kind === 'ldap') {
      void runCredentialSignin(async () =>
        settle(
          await ssoApi.signinLdap({
            providerId: provider.id,
            username: email,
            password,
            remember,
          }),
          { remember, last: { channel: { kind: 'provider', id: provider.id }, account: email } },
        ),
      );
      return;
    }
    setSsoError(null);
    keepSsoTrip(remember, provider.id);
    window.location.assign(ssoApi.buildLoginUrl(provider, requestedNext ?? undefined));
  };

  // Back from the identity provider: drop the code from the address, then trade it
  // for the session (or for the reason the sign-in failed). The ref keeps a second
  // effect run in development from spending the single-use code twice.
  const ssoExchanged = React.useRef(false);
  React.useEffect(() => {
    if (!ssoCode || ssoExchanged.current) return;
    ssoExchanged.current = true;
    const cleanUrl = new URL(window.location.href);
    cleanUrl.searchParams.delete('sso');
    window.history.replaceState(
      window.history.state,
      '',
      `${cleanUrl.pathname}${cleanUrl.search}`,
    );
    const trip = takeSsoTrip();
    setBusy(true);
    ssoApi
      .exchange(ssoCode, trip.remember)
      .then((res) =>
        settle(res, {
          remember: trip.remember,
          ...(res.next ? { next: res.next } : {}),
          ...(trip.providerId
            ? { last: { channel: { kind: 'provider' as const, id: trip.providerId } } }
            : {}),
        }),
      )
      // Shown in the card rather than as a toast: the interface language may still
      // be settling when the answer arrives, and the reason should stay readable.
      .catch((err: unknown) => setSsoError(toApiError(err).message))
      .finally(() => setBusy(false));
    // completeSignin reads the latest render's values; the exchange runs once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ssoCode]);

  const submitSignin = async () => {
    await runCredentialSignin(
      async () =>
        settle(await auth.signin({ email, password, remember }), {
          remember,
          last: { channel: { kind: 'password' }, account: email },
        }),
      t(passkeyAvailable ? 'shell:signin.limited_hint_passkey' : 'shell:signin.limited_hint'),
    );
  };

  const onEmailStep =
    view === 'signin' && steps.step.name === 'identifier' && !steps.step.directory;
  const passkeyAutofill = passkeyAvailable && onEmailStep;

  const passkeySignin = usePasskeySignin({
    enabled: passkeyAvailable && (onEmailStep || (view === 'signin' && steps.step.name === 'channels')),
    autofill: passkeyAutofill,
    remember,
    onResult: (res, keep) =>
      settle(res, { remember: keep, last: { channel: { kind: 'passkey' } } }),
    onSsoOnly: steps.restart,
  });

  /** The address someone typed comes along to sign up; the one that signed in last isn't new. */
  const signupEmail =
    view === 'signin' &&
    (steps.step.name === 'identifier' || steps.step.name === 'password') &&
    !steps.step.directory &&
    email !== rememberedEmail(last)
      ? email.trim()
      : '';

  const chooseChannel = (row: ChannelRow) => {
    if (row.kind === 'passkey') {
      void passkeySignin.signIn();
    } else if (row.kind === 'account') {
      setEmail(row.account);
      setPassword('');
      steps.skipToPassword(null);
    } else if (row.provider.kind !== 'ldap') {
      beginSso(row.provider);
    } else {
      setEmail(row.account ?? '');
      setPassword('');
      if (row.account) steps.skipToPassword(row.provider);
      else steps.askIdentifier(row.provider);
    }
  };

  /** The form's own button: on to the next question, or, with the password in, signing in. */
  const submitStep = () => {
    const { step } = steps;

    if (step.name === 'channels') {
      // A directory's username, from a row tried before, has no place in the email field.
      setEmail((typed) => (typed.includes('@') ? typed : rememberedEmail(last)));
      steps.askIdentifier(null);
    } else if (step.name === 'identifier') {
      setPassword('');
      steps.askPassword(step.directory);
    } else if (step.name === 'password') {
      if (step.directory) beginSso(step.directory);
      else void submitSignin();
    }
  };

  const sendLinkCode = async () => {
    if (!link) return;
    setLinkSending(true);
    try {
      await ssoApi.sendLinkCode(link.info.code, i18n.resolvedLanguage ?? i18n.language);
      setLinkCode('');
      setLinkCodeSentAt(Date.now());
    } catch (err) {
      toast.error(t('shell:signin.link.send_failed'), {
        description: toApiError(err).message,
      });
    } finally {
      setLinkSending(false);
    }
  };

  const submitLink = async () => {
    if (!link) return;
    setBusy(true);
    try {
      const res = await ssoApi.confirmLink(
        link.info.code,
        linkMethod === 'email' ? { emailCode: linkCode } : { password },
        link.options.remember,
      );
      await settle(res, link.options);
    } catch (err) {
      const e = toApiError(err);
      toast.error(t('errors:sign_in_failed'), { description: signinLimitWait(e, t) ?? e.message });
    } finally {
      setBusy(false);
    }
  };

  const recovery = usePasswordRecovery({
    email,
    token: resetToken,
    password,
    confirmPassword,
    setBusy,
    onSent: () => setView('forgot-password-sent'),
    onReset: () => {
      setPassword('');
      setConfirmPassword('');
      setView('signin');
      nav('/signin', { replace: true });
    },
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (busy || linkSending) return;
    if (view === 'signin') submitStep();
    if (view === 'forgot-password') void recovery.askForLink();
    if (view === 'reset-password') void recovery.setNewPassword();
    // The first code is asked for with the form's own button; the rest with "resend".
    if (view === 'sso-link') {
      void (linkMethod === 'email' && linkCodeSentAt === null ? sendLinkCode() : submitLink());
    }
    if (view === 'mfa') secondStep.submit();
    if (view === 'mfa-enroll') enrollSubmit.current?.();
  };

  const backToSignin = () => {
    setPassword('');
    setConfirmPassword('');
    setLink(null);
    secondStep.reset();
    setEnroll(null);
    setView('signin');
    if (resetToken) nav('/signin', { replace: true });
  };

  return (
    <SigninShell
      view={view}
      email={email}
      onEmailChange={(value) => {
        setEmail(value);
        steps.stayOnEmail();
      }}
      password={password}
      onPasswordChange={setPassword}
      confirmPassword={confirmPassword}
      onConfirmPasswordChange={setConfirmPassword}
      remember={remember}
      onRememberChange={setRemember}
      busy={busy}
      onSubmit={submit}
      onOpenForgotPassword={() => {
        setPassword('');
        setView('forgot-password');
      }}
      onBackToSignin={backToSignin}
      signin={{
        step: steps.step,
        rows,
        canGoBack: steps.canGoBack,
        onBack: steps.back,
        onChoose: chooseChannel,
        passkeyBusy: passkeySignin.busy,
        passkeyAutofill,
      }}
      ssoError={ssoError}
      ssoLink={
        link && {
          link: link.info,
          method: linkMethod,
          onMethodChange: setLinkMethod,
          emailCode: linkCode,
          onEmailCodeChange: setLinkCode,
          password,
          onPasswordChange: setPassword,
          codeSentAt: linkCodeSentAt,
          sending: linkSending,
          onResendCode: () => void sendLinkCode(),
        }
      }
      mfa={secondStep.panel}
      enroll={
        enroll && {
          organizationName: enroll.required.organizationName,
          panel: (
            <EnrollPanel
              enroll={enroll.required}
              submitRef={enrollSubmit}
              onFinished={(res) => {
                if (res.login) void completeSignin(res.login, enroll.options);
              }}
              onExpired={() => {
                toast.error(t('errors:sign_in_failed'), {
                  description: t('shell:signin.enroll.expired'),
                });
                backToSignin();
              }}
              onBack={backToSignin}
            />
          ),
        }
      }
      buildVersion={instanceQuery.data?.version}
      signupEnabled={instanceQuery.data?.signupEnabled ?? false}
      onSignup={() => nav('/signup', signupEmail ? { state: { email: signupEmail } } : undefined)}
    />
  );
}
