import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { SigninShell, type SigninShellProps } from '@/shell/signin/SigninShell';

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

  it('sits on one full-height stage with a single centred column', () => {
    render(<SigninShell {...signinProps()} />);

    const stage = screen.getByTestId('auth-stage');
    // The visible viewport on phones; a short window scrolls instead of clipping the form.
    expect(stage.className).toContain('h-[100dvh]');
    expect(stage.className).toContain('overflow-y-auto');
    expect(stage.className).not.toContain('min-w-[1280px]');

    expect(screen.getByRole('main').className).toContain('max-w-[440px]');

    // One form card: no split panel, nothing drawn on a canvas.
    const card = screen.getByTestId('signin-card');
    expect(card.tagName).toBe('FORM');
    expect(card.className).toContain('w-full');
    expect(document.querySelector('canvas')).toBeNull();
  });

  it('leads with the brand and one sentence, and nothing else above the form', () => {
    render(<SigninShell {...signinProps()} />);

    const logo = screen.getByTestId('signin-logo');
    expect(logo.querySelector('svg')?.getAttribute('width')).toBe('36');
    expect(logo.textContent).toContain('MoleSignal');

    expect(screen.getByText('All signals,')).not.toBeNull();
    expect(screen.getByText('one query,')).not.toBeNull();
    expect(screen.getByText('one timeline.')).not.toBeNull();
    expect(
      screen.getByText('Observe everything. Understand what matters.'),
    ).not.toBeNull();

    // The capability blurb and the invented terminal readout are gone.
    expect(screen.queryByText(/unified in one query plane/)).toBeNull();
    expect(screen.queryByText(/molesignal status/)).toBeNull();
    expect(screen.queryByText(/18 services/)).toBeNull();
  });

  it('uses the shared control surface for fields and a full-width primary action', () => {
    render(
      <SigninShell
        {...signinProps({
          ssoProviders: [
            { id: 'workspace-oidc', name: 'Workspace SSO', kind: 'oidc' },
          ],
        })}
      />,
    );

    expect(screen.getByRole('heading', { level: 1, name: 'Sign in' })).not.toBeNull();
    for (const name of ['Email', 'Password']) {
      const input = screen.getByLabelText(name);
      // 44px on a phone, 16px text so iOS does not zoom; 40px from `sm` up.
      expect(input.className).toContain('h-11');
      expect(input.className).toContain('sm:h-10');
      expect(input.className).toContain('text-[16px]');
      // The fill is the field's only edge.
      expect(input.className).toContain('bg-[var(--control-surface)]');
      expect(input.className).not.toMatch(/\bborder\b/);
    }

    const submit = screen.getByRole('button', { name: 'Sign in →' }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    expect(submit.className).toContain('auth-primary-button');
    expect(submit.className).toContain('sm:h-10');
    expect(
      screen.getByRole('button', { name: 'Sign in with Workspace SSO' }),
    ).not.toBeNull();
  });

  it('shows placeholders that cannot be mistaken for typed values', () => {
    render(<SigninShell {...signinProps()} />);

    expect(screen.getByLabelText('Email').getAttribute('placeholder')).toBe(
      'name@company.com',
    );
    expect(screen.getByLabelText('Password').getAttribute('placeholder')).toBe(
      'Enter your password',
    );
  });

  it('enables the primary action once both fields are filled', () => {
    render(
      <SigninShell {...signinProps({ email: 'a@example.com', password: 'secret-pw' })} />,
    );
    expect(
      (screen.getByRole('button', { name: 'Sign in →' }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });

  it('runs the pulse inside the submit button while the request is in flight', () => {
    render(
      <SigninShell {...signinProps({ email: 'a@example.com', password: 'secret-pw', busy: true })} />,
    );

    const submit = screen.getByRole('button', { name: /Signing in/ }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    expect(submit.querySelector('path.ms-pulse-travel')).not.toBeNull();
  });

  it('only renders the account entry when signup is enabled', () => {
    const { rerender } = render(<SigninShell {...signinProps()} />);

    expect(screen.queryByText('No account yet?')).toBeNull();
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

  it('shows the product version and nothing about the release channel', () => {
    const { rerender } = render(
      <SigninShell {...signinProps({ buildVersion: undefined })} />,
    );

    expect(screen.queryByTestId('signin-build-info')).toBeNull();

    rerender(<SigninShell {...signinProps({ buildVersion: '26.0.0.0' })} />);
    expect(screen.getByTestId('signin-build-info').textContent).toBe('v26.0.0.0');
    expect(screen.queryByText(/alpha|beta|stable|\brc\b/i)).toBeNull();
  });

  it('renders the Chinese brand copy without the dropped capability blurb', async () => {
    await i18n.changeLanguage('zh-cn');
    render(<SigninShell {...signinProps()} />);

    expect(screen.getByText('所有信号，')).not.toBeNull();
    expect(screen.getByText('一次查询，')).not.toBeNull();
    expect(screen.getByText('一条时间线。')).not.toBeNull();
    expect(screen.getByText('观察一切，洞悉关键所在。')).not.toBeNull();
    expect(screen.queryByText('日志、指标、追踪、分析、APM 和 RUM')).toBeNull();
    expect(screen.queryByText('统一于单一查询平台之下。')).toBeNull();
  });

  it('keeps the same card for password recovery', () => {
    const onBack = vi.fn();
    const { rerender } = render(
      <SigninShell
        {...signinProps({
          view: 'forgot-password',
          email: 'a@example.com',
          onBackToSignin: onBack,
        })}
      />,
    );

    expect(screen.getByRole('heading', { level: 1, name: 'Reset password' })).not.toBeNull();
    expect(
      (screen.getByRole('button', { name: 'Send reset link →' }) as HTMLButtonElement).disabled,
    ).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Back to sign in' }));
    expect(onBack).toHaveBeenCalledOnce();

    rerender(
      <SigninShell {...signinProps({ view: 'forgot-password-sent', email: 'a@example.com' })} />,
    );
    expect(screen.getByRole('heading', { level: 1, name: 'Check your email' })).not.toBeNull();
    expect(screen.getByTestId('signin-card').tagName).toBe('FORM');
  });

  it('flags a password that does not match its confirmation', () => {
    render(
      <SigninShell
        {...signinProps({
          view: 'reset-password',
          password: 'long-enough-1',
          confirmPassword: 'long-enough-2',
        })}
      />,
    );

    expect(screen.getByRole('alert').textContent).toBe('The passwords do not match.');
    expect(
      (screen.getByRole('button', { name: 'Update password →' }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});
