import { useMutation, useQuery } from '@tanstack/react-query';
import { MailCheck } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import * as auth from '@/api/auth';
import * as instanceApi from '@/api/instance';
import { toApiError } from '@/lib/http';
import { BackButton, Field, PrimaryButton, StepHeader } from '@/shell/signin/AuthControls';
import { AuthStage } from '@/shell/signin/AuthStage';
import { BrandHeader } from '@/shell/signin/BrandHeader';
import { toast } from '@/shell/ui/sonner';
import { type AuthContextInput, useAuthStore } from '@/stores/auth';

/** The token of the link this page was opened with, taken out of the address. */
function useSignupLinkToken() {
  const [token, setToken] = React.useState(() =>
    (new URLSearchParams(window.location.hash.replace(/^#/, '')).get('signupToken') ?? '').trim(),
  );

  React.useEffect(() => {
    if (!token) return;
    const cleanUrl = new URL(window.location.href);
    cleanUrl.hash = '';
    window.history.replaceState(
      window.history.state,
      '',
      `${cleanUrl.pathname}${cleanUrl.search}`,
    );
  }, [token]);

  return [token, () => setToken('')] as const;
}

/**
 * 自助注册页（公开路由 `/signup`）。仅当实例开启注册时，signin 页才提供入口。这里不判断邮箱有没有
 * 账号：回答一律一样，未登录的人探测不出账号是否存在。
 *
 * - 实例能发邮件：只问邮箱，然后提示去看邮箱。本人打开邮件里的链接（`#signupToken=`）回到这里，才
 *   设名字和密码。
 * - 实例不能发邮件：先问邮箱，再问名字和密码；从登录页的邮箱那一步过来时带着邮箱，直接到第二步。提交后
 *   提示待审批。
 *
 * 用链接建好账号之后：active → 直接建立会话进首页；pending → 提示待审批；signin_required → 去登录。
 */
export function Signup() {
  const { t, i18n } = useTranslation(['shell', 'common']);
  const nav = useNavigate();
  const setSession = useAuthStore((s) => s.setSession);
  const location = useLocation();
  const emailFromSignin = (location.state as { email?: string } | null)?.email ?? '';
  const [token, forgetToken] = useSignupLinkToken();
  const [email, setEmail] = React.useState(emailFromSignin);
  const [step, setStep] = React.useState<'email' | 'details' | 'sent'>(
    emailFromSignin ? 'details' : 'email',
  );
  const [displayName, setDisplayName] = React.useState('');
  const [password, setPassword] = React.useState('');
  const [linkUsedUp, setLinkUsedUp] = React.useState(false);

  const instanceQuery = useQuery({
    queryKey: ['instance'],
    queryFn: () => instanceApi.get(),
    retry: false,
    staleTime: 60_000,
    enabled: !token,
  });
  const verifiesEmail = instanceQuery.data?.signupVerifiesEmail === true;

  const linkQuery = useQuery({
    queryKey: ['signup', 'link', token],
    queryFn: () => auth.verifySignupLink(token),
    enabled: Boolean(token),
    retry: false,
    gcTime: 0,
  });
  const linkEmail = linkQuery.data?.email;
  const linkExpired = Boolean(token) && (linkQuery.isError || linkUsedUp);
  const linkChecking = Boolean(token) && linkQuery.isLoading;

  const showPending = () => {
    toast.success(t('shell:signup.pending_title'), {
      description: t('shell:signup.pending_desc'),
    });
    nav('/signin', { replace: true });
  };

  const ask = useMutation({
    mutationFn: () =>
      verifiesEmail
        ? auth.requestSignupLink({
            email: email.trim(),
            locale: i18n.resolvedLanguage ?? i18n.language,
          })
        : auth.applyForSignup({ email: email.trim(), displayName: displayName, password }),
    onSuccess: (res) => {
      if (res.status === 'verify_email') setStep('sent');
      else showPending();
    },
    onError: (err) =>
      toast.error(t('shell:signup.failed'), { description: toApiError(err).message }),
  });

  const complete = useMutation({
    mutationFn: () => auth.completeSignup({ token, displayName: displayName, password }),
    onSuccess: (res) => {
      if (res.status === 'signin_required') {
        toast.success(t('shell:signup.signin_required_title'), {
          description: t('shell:signup.signin_required_desc'),
        });
        nav('/signin', { replace: true });
        return;
      }
      if (res.status !== 'active') {
        showPending();
        return;
      }
      const ctx: AuthContextInput = {
        userId: res.userId,
        orgId: res.orgId ?? '',
        displayRole: res.displayRole ?? '',
        roles: res.roles,
      };
      if (res.email) ctx.email = res.email;
      if (res.displayName) ctx.displayName = res.displayName;
      if (res.orgName) ctx.orgName = res.orgName;
      setSession(ctx, false);
      nav('/home', { replace: true });
    },
    onError: (err) => {
      const e = toApiError(err);
      if (e.status === 400 && e.message.includes('signup link')) setLinkUsedUp(true);
      else toast.error(t('shell:signup.failed'), { description: e.message });
    },
  });

  const startAgain = () => {
    forgetToken();
    setLinkUsedUp(false);
    setStep('email');
  };

  const fromLink = Boolean(token) && linkEmail !== undefined;
  const asksDetails = fromLink || (!token && step === 'details' && !verifiesEmail);
  const sent = !token && step === 'sent';
  const pending = ask.isPending || complete.isPending;
  const passwordShort = [...password].length < 8;

  const heading = linkExpired
    ? t('shell:signup.link_expired_title')
    : sent
      ? t('shell:signup.check_email_title')
      : t('shell:signup.title');

  const subtitle = linkExpired
    ? t('shell:signup.link_expired_subtitle')
    : sent
      ? t('shell:signup.check_email_subtitle', { email: email.trim() })
      : linkChecking
        ? t('shell:signup.link_checking')
        : asksDetails
          ? t('shell:signup.details_subtitle')
          : t('shell:signup.subtitle');

  const account = linkEmail ?? email;

  return (
    <AuthStage>
      <BrandHeader />

      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (pending) return;
          if (linkExpired) startAgain();
          else if (fromLink) complete.mutate();
          else if (asksDetails || verifiesEmail) ask.mutate();
          else setStep('details');
        }}
        className="ms-auth-card mt-9 flex w-full min-w-0 flex-col gap-5 rounded-2xl border border-bd-0 bg-bg-1 p-6 shadow-login sm:p-8"
        data-testid="signup-card"
      >
        <div>
          <h1 className="m-0 font-sans text-xl font-display tracking-tight text-tx-0">{heading}</h1>
          <div className="mt-1.5 text-xs text-tx-2">{subtitle}</div>
        </div>

        {sent && (
          <div className="flex flex-col">
            <div className="flex h-11 w-11 items-center justify-center rounded-lg bg-indigo-dim text-indigo-soft">
              <MailCheck size={20} strokeWidth={1.8} aria-hidden />
            </div>
            <div className="mt-4 font-sans text-xs leading-relaxed text-tx-1">
              {t('shell:signup.check_email_detail')}
            </div>
            <BackButton onClick={() => nav('/signin')} className="mt-6">
              {t('shell:signup.back_to_signin')}
            </BackButton>
          </div>
        )}

        {linkExpired && (
          <PrimaryButton disabled={false}>{`${t('shell:signup.start_again')} →`}</PrimaryButton>
        )}

        {!sent && !linkExpired && !linkChecking && (
          <div className="flex flex-col gap-4">
            <StepHeader
              onBack={
                fromLink
                  ? undefined
                  : asksDetails
                    ? () => setStep('email')
                    : () => nav('/signin')
              }
              backLabel={t('shell:signin.back')}
              {...(asksDetails && account ? { account } : {})}
            />
            {asksDetails ? (
              <>
                <input
                  type="email"
                  name="username"
                  autoComplete="username"
                  value={account}
                  readOnly
                  tabIndex={-1}
                  aria-hidden="true"
                  className="sr-only"
                />
                <Field
                  label={t('shell:signup.display_name')}
                  value={displayName}
                  onChange={setDisplayName}
                  placeholder={t('shell:signup.display_name_placeholder')}
                  name="name"
                  autoComplete="name"
                  autoFocus
                />
                <div className="flex flex-col gap-1.5">
                  <Field
                    label={t('common:labels.password')}
                    value={password}
                    onChange={setPassword}
                    type="password"
                    name="password"
                    autoComplete="new-password"
                    revealable
                  />
                  <div className="font-sans text-xs text-tx-2">
                    {t('shell:signin.password_hint')}
                  </div>
                </div>
                <PrimaryButton
                  disabled={pending || !displayName.trim() || passwordShort}
                  busy={pending}
                >
                  {pending ? t('shell:signup.submitting') : `${t('shell:signup.submit')} →`}
                </PrimaryButton>
              </>
            ) : (
              <>
                <Field
                  label={t('common:labels.email')}
                  value={email}
                  onChange={setEmail}
                  placeholder={t('shell:signin.email_placeholder')}
                  type="email"
                  name="username"
                  autoComplete="username"
                  autoFocus
                />
                <PrimaryButton disabled={pending || !email.trim()} busy={pending}>
                  {`${t('shell:signin.continue')} →`}
                </PrimaryButton>
              </>
            )}
          </div>
        )}
      </form>
    </AuthStage>
  );
}
