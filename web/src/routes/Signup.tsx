import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as auth from '@/api/auth';
import { toApiError } from '@/lib/http';
import { uiLabelClass } from '@/shell/chrome';
import { LogoMark } from '@/shell/LogoMark';
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
    <div className="grid min-h-screen min-w-[1280px] place-items-center bg-bg-0 p-6">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit.mutate();
        }}
        className="flex w-[420px] animate-fade-in flex-col gap-4 rounded-xl border border-bd-0 bg-bg-1 p-10 shadow-login"
      >
        <div className="flex items-center gap-2.5">
          <LogoMark size={28} />
          <span className="font-sans text-base font-strong tracking-tight">
            {t('common:app_name')}
          </span>
        </div>
        <div>
          <h1 className="m-0 font-sans text-[22px] tracking-tight text-tx-0">
            {t('shell:signup.title')}
          </h1>
          <div className="mt-1.5 text-xs text-tx-2">{t('shell:signup.subtitle')}</div>
        </div>
        <div className="flex flex-col gap-3.5">
          <Field
            label={t('common:labels.email')}
            value={email}
            onChange={setEmail}
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
          <button
            type="submit"
            disabled={submit.isPending || !email || !displayName || !password}
            className="auth-primary-button mt-2 flex h-9 items-center justify-center rounded-md font-sans text-xs font-bold tracking-wide text-white focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
          >
            {submit.isPending ? t('shell:signup.submitting') : t('shell:signup.submit')}
          </button>
          <button
            type="button"
            onClick={() => nav('/signin')}
            className="cursor-pointer text-center font-sans text-xs text-blue-soft"
          >
            {t('shell:signup.back_to_signin')}
          </button>
        </div>
      </form>
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  type = 'text',
  autoFocus,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
  autoFocus?: boolean;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className={uiLabelClass}>{label}</span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        autoFocus={autoFocus}
        aria-label={label}
        className="h-9 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-xs text-tx-0 placeholder:text-tx-3 focus:outline-none"
      />
    </label>
  );
}
