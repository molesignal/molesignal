import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { SigninShell, type SigninShellProps } from '@/shell/SigninShell';

function signinProps(
  overrides: Partial<SigninShellProps> = {},
): SigninShellProps {
  return {
    view: 'signin',
    email: '',
    onEmailChange: vi.fn(),
    password: '',
    onPasswordChange: vi.fn(),
    confirmPassword: '',
    onConfirmPasswordChange: vi.fn(),
    remember: false,
    onRememberChange: vi.fn(),
    busy: false,
    onSubmit: vi.fn(),
    onOpenForgotPassword: vi.fn(),
    onBackToSignin: vi.fn(),
    ssoProviders: [],
    onBeginSso: vi.fn(),
    buildVersion: '26.0.0.0',
    releaseChannel: 'stable',
    ...overrides,
  };
}

describe.sequential('SigninShell', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(() => {
    cleanup();
  });

  it('uses the 880px desktop card and 380px brand panel', () => {
    render(<SigninShell {...signinProps()} />);

    const card = screen.getByTestId('signin-card');
    const brand = screen.getByTestId('signin-brand');
    const form = screen.getByTestId('signin-form');

    expect(card.className).toContain('max-w-[880px]');
    expect(card.className).toContain('lg:min-h-[520px]');
    expect(card.className).toContain(
      'lg:grid-cols-[380px_minmax(0,1fr)]',
    );
    expect(card.className).toContain('grid-cols-1');
    expect(card.parentElement?.className).not.toContain('min-w-[1280px]');
    expect(card.parentElement?.className).toContain('h-[100dvh]');
    expect(card.parentElement?.className).toContain('overflow-y-auto');
    expect(brand.className).toContain('sm:p-8');
    expect(brand.className).toContain('lg:pb-12');
    expect(form.className).toContain('sm:p-12');
    expect(screen.getByTestId('signin-brand-content').className).toContain(
      'lg:mt-[88px]',
    );
    expect(screen.getByTestId('signin-brand-content').className).toContain(
      'mt-9',
    );
  });

  it('renders compact controls and the aligned brand content group', () => {
    render(
      <SigninShell
        {...signinProps({
          ssoProviders: [
            { id: 'workspace-oidc', name: 'Workspace SSO', kind: 'oidc' },
          ],
        })}
      />,
    );

    expect(screen.getByRole('heading', { name: 'Sign in' }).className).toContain(
      'text-[22px]',
    );
    for (const name of ['Email', 'Password']) {
      const input = screen.getByLabelText(name);
      expect(input.className).toContain('h-11');
      expect(input.className).toContain('sm:h-9');
      expect(input.className).toContain('text-[16px]');
      expect(input.className).toContain('sm:text-xs');
    }
    expect(screen.getByRole('button', { name: 'Sign in →' }).className).toContain(
      'sm:h-9',
    );
    const submit = screen.getByRole('button', {
      name: 'Sign in →',
    }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    expect(submit.className).toContain('auth-primary-button');
    expect(
      screen.getByRole('button', { name: 'Sign in with Workspace SSO' })
        .className,
    ).toContain('h-8');
    const signinLogo = screen.getByTestId('signin-logo');
    expect(signinLogo.className).toContain('gap-3');
    expect(signinLogo.className).toContain('translate-y-[2px]');
    expect(signinLogo.querySelector('svg')?.getAttribute('width')).toBe('40');
    expect(signinLogo.querySelector('span')?.className).toContain('text-[22px]');
    expect(screen.getByText('All signals,')).not.toBeNull();
    expect(screen.getByText('one query,')).not.toBeNull();
    expect(screen.getByText('one timeline.')).not.toBeNull();
    expect(
      screen.getByText('Observe everything. Understand what matters.'),
    ).not.toBeNull();
    expect(
      screen.getByText('Logs, metrics, traces, profiles, APM, and RUM—'),
    ).not.toBeNull();
    expect(
      screen.getByText('unified in one query plane.'),
    ).not.toBeNull();
    expect(screen.getByTestId('signin-brand').textContent).toContain(
      '$ molesignal status',
    );
    expect(screen.getByTestId('signin-brand').textContent).toContain(
      '18 services · 2 warnings',
    );
    expect(screen.getByText('All telemetry pipelines ready')).not.toBeNull();
    expect(screen.getByTestId('signin-status').className).toContain('mt-9');
    expect(screen.getByTestId('signin-build-info').textContent).toBe(
      'v26.0.0.0 · stable',
    );
    expect(screen.getByTestId('signin-build-info').className).toContain(
      'text-left',
    );
    expect(screen.getByTestId('signin-build-info').className).toContain(
      'text-xs',
    );
    expect(screen.getByTestId('signin-build-info').className).toContain('mt-6');
    expect(screen.getByTestId('signin-build-info').className).toContain(
      'lg:mt-auto',
    );
  });

  it('only renders the account entry when signup is enabled', () => {
    const { rerender } = render(<SigninShell {...signinProps()} />);

    expect(screen.queryByText('No account yet?')).toBeNull();
    expect(screen.queryByText('Request trial →')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Sign up →' })).toBeNull();

    const onSignup = vi.fn();
    rerender(
      <SigninShell
        {...signinProps({
          signupEnabled: true,
          onSignup,
        })}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Sign up →' }));
    expect(onSignup).toHaveBeenCalledTimes(1);
  });

  it('only renders the backend version after its release channel is available', () => {
    const { rerender } = render(
      <SigninShell
        {...signinProps({ buildVersion: undefined, releaseChannel: undefined })}
      />,
    );

    expect(screen.queryByTestId('signin-build-info')).toBeNull();

    rerender(
      <SigninShell
        {...signinProps({ buildVersion: '26.0.0.0', releaseChannel: 'alpha' })}
      />,
    );
    expect(screen.getByTestId('signin-build-info').textContent).toBe(
      'v26.0.0.0 · alpha',
    );
  });

  it('renders the requested Chinese brand copy', async () => {
    await i18n.changeLanguage('zh-cn');
    render(<SigninShell {...signinProps()} />);

    expect(screen.getByText('所有信号，')).not.toBeNull();
    expect(screen.getByText('一次查询，')).not.toBeNull();
    expect(screen.getByText('一条时间线。')).not.toBeNull();
    expect(screen.getByText('观察一切，洞悉关键所在。')).not.toBeNull();
    expect(screen.getByText('日志、指标、追踪、分析、APM 和 RUM')).not.toBeNull();
    expect(screen.getByText('统一于单一查询平台之下。')).not.toBeNull();
  });
});
