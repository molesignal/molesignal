import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as auth from '@/api/auth';
import { toApiError } from '@/lib/http';
import { Field, PrimaryButton } from '@/shell/signin/AuthControls';
import { AuthStage } from '@/shell/signin/AuthStage';
import { BrandHeader } from '@/shell/signin/BrandHeader';
import { toast } from '@/shell/ui/sonner';
import { type AuthContextInput, useAuthStore } from '@/stores/auth';

/**
 * 自助注册页（公开路由 `/signup`）。仅当实例开启注册时，signin 页才提供入口。
 * 提交后：active → 直接建立会话进首页；pending → 提示待审批并回到 signin。
 */
export function Signup() {
  const { t } = useTranslation(['shell', 'common']);
  const nav = useNavigate();
  const setSession = useAuthStore((s) => s.setSession);
  const [email, setEmail] = React.useState('');
  const [displayName, setDisplayName] = React.useState('');
  const [password, setPassword] = React.useState('');

  const submit = useMutation({
    mutationFn: () => auth.signup({ email, display_name: displayName, password }),
    onSuccess: (res) => {
      if (res.status === 'pending' || !res.token) {
        toast.success(t('shell:signup.pending_title'), {
          description: t('shell:signup.pending_desc'),
        });
        nav('/signin', { replace: true });
        return;
      }
      const ctx: AuthContextInput = {
        user_id: res.user_id,
        org_id: res.org_id ?? '',
        display_role: res.display_role ?? '',
        roles: res.roles,
      };
      if (res.email) ctx.email = res.email;
      if (res.display_name) ctx.display_name = res.display_name;
      if (res.org_name) ctx.org_name = res.org_name;
      setSession(res.token, ctx);
      nav('/home', { replace: true });
    },
    onError: (err) =>
      toast.error(t('shell:signup.failed'), { description: toApiError(err).message }),
  });

  return (
    <AuthStage>
      <BrandHeader />

      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit.mutate();
        }}
        className="ms-auth-card mt-9 flex w-full flex-col gap-5 rounded-2xl border border-bd-0 bg-bg-1 p-6 shadow-login sm:p-8"
      >
        <div>
          <h1 className="m-0 font-sans text-xl font-display tracking-tight text-tx-0">
            {t('shell:signup.title')}
          </h1>
          <div className="mt-1.5 text-xs text-tx-2">{t('shell:signup.subtitle')}</div>
        </div>
        <div className="flex flex-col gap-4">
          <Field
            label={t('common:labels.email')}
            value={email}
            onChange={setEmail}
            placeholder={t('shell:signin.email_placeholder')}
            type="email"
            autoFocus
          />
          <Field
            label={t('shell:signup.display_name')}
            value={displayName}
            onChange={setDisplayName}
          />
          <Field
            label={t('common:labels.password')}
            value={password}
            onChange={setPassword}
            type="password"
          />
          <PrimaryButton
            disabled={submit.isPending || !email || !displayName || !password}
            busy={submit.isPending}
          >
            {submit.isPending ? t('shell:signup.submitting') : t('shell:signup.submit')}
          </PrimaryButton>
          <button
            type="button"
            onClick={() => nav('/signin')}
            className="cursor-pointer text-center font-sans text-xs text-[var(--blue-ink)] hover:underline"
          >
            {t('shell:signup.back_to_signin')}
          </button>
        </div>
      </form>
    </AuthStage>
  );
}
