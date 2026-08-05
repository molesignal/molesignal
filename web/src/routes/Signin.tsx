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
import { SigninShell, type SigninView } from '@/shell/SigninShell';
import { toast } from '@/shell/ui/sonner';
import { type AuthContextInput, useAuthStore } from '@/stores/auth';

export function Signin() {
  const { t, i18n } = useTranslation(['errors', 'shell']);
  const nav = useNavigate();
  const [params] = useSearchParams();
  const requestedNext = params.get('next');
  const next = requestedNext ?? '/';
  const resetTokenFromUrl =
    params.get('reset_token') ??
    new URLSearchParams(window.location.hash.replace(/^#/, '')).get('reset_token') ??
    '';
  const [resetToken] = React.useState(resetTokenFromUrl.trim());
  const setSession = useAuthStore((s) => s.setSession);

  const [email, setEmail] = React.useState('');
  const [password, setPassword] = React.useState('');
  const [confirmPassword, setConfirmPassword] = React.useState('');
  const [remember, setRemember] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [view, setView] = React.useState<SigninView>(
    resetToken ? 'reset-password' : 'signin',
  );

  React.useEffect(() => {
    if (!resetToken) return;
    const cleanUrl = new URL(window.location.href);
    cleanUrl.searchParams.delete('reset_token');
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
  const ssoProviders = ssoQuery.data ?? [];

  // 公开实例信息：signup_enabled 决定是否显示「注册」入口。
  const instanceQuery = useQuery({
    queryKey: ['instance'],
    queryFn: () => instanceApi.get(),
    retry: false,
    staleTime: 60_000,
  });

  const completeSignin = async (res: auth.SigninResponse) => {
    const nextCtx: AuthContextInput = {
      user_id: res.user_id,
      org_id: res.org_id,
      display_role: res.display_role,
      roles: res.roles,
    };
    if (res.email) nextCtx.email = res.email;
    if (res.display_name) nextCtx.display_name = res.display_name;
    if (res.org_name) nextCtx.org_name = res.org_name;
    setSession(res.token, nextCtx, remember);
    let destination = next;
    if (!requestedNext) {
      try {
        const preferences = await meApi.preferences();
        destination = resolveDefaultHomeRoute(
          preferences.default_home_route,
          res.user_id,
          res.org_id,
        );
      } catch {
        destination = '/home';
      }
    }
    nav(destination, { replace: true });
  };

  const runCredentialSignin = async (request: () => Promise<auth.SigninResponse>) => {
    setBusy(true);
    try {
      await completeSignin(await request());
    } catch (err) {
      const e = toApiError(err);
      toast.error(t('errors:sign_in_failed'), { description: e.message });
    } finally {
      setBusy(false);
    }
  };

  const beginSso = (provider: ssoApi.PublicSsoProvider) => {
    if (provider.kind === 'ldap') {
      void runCredentialSignin(() =>
        ssoApi.signinLdap({
          provider_id: provider.id,
          username: email,
          password,
        }),
      );
      return;
    }
    window.location.assign(ssoApi.buildLoginUrl(provider, next));
  };

  const submitSignin = async () => {
    await runCredentialSignin(() => auth.signin({ email, password }));
  };

  const submitForgotPassword = async () => {
    setBusy(true);
    try {
      await auth.forgotPassword({
        email,
        locale: i18n.resolvedLanguage ?? i18n.language,
      });
      setView('forgot-password-sent');
    } catch (err) {
      const apiError = toApiError(err);
      toast.error(t('shell:signin.forgot_failed'), { description: apiError.message });
    } finally {
      setBusy(false);
    }
  };

  const submitResetPassword = async () => {
    if (password !== confirmPassword) {
      toast.error(t('shell:signin.password_mismatch'));
      return;
    }
    setBusy(true);
    try {
      await auth.resetPassword({ token: resetToken, password });
      toast.success(t('shell:signin.reset_success'));
      setPassword('');
      setConfirmPassword('');
      setView('signin');
      nav('/signin', { replace: true });
    } catch (err) {
      const apiError = toApiError(err);
      toast.error(t('shell:signin.reset_failed'), {
        description:
          apiError.status === 400 ? t('shell:signin.reset_invalid') : apiError.message,
      });
    } finally {
      setBusy(false);
    }
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (busy) return;
    if (view === 'signin') void submitSignin();
    if (view === 'forgot-password') void submitForgotPassword();
    if (view === 'reset-password') void submitResetPassword();
  };

  const backToSignin = () => {
    setPassword('');
    setConfirmPassword('');
    setView('signin');
    if (resetToken) nav('/signin', { replace: true });
  };

  return (
    <SigninShell
      view={view}
      email={email}
      onEmailChange={setEmail}
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
      ssoProviders={ssoProviders}
      onBeginSso={beginSso}
      buildVersion={instanceQuery.data?.version}
      releaseChannel={instanceQuery.data?.release_channel}
      signupEnabled={instanceQuery.data?.signup_enabled ?? false}
      onSignup={() => nav('/signup')}
    />
  );
}
