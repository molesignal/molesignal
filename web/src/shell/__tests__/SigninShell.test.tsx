import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { SigninShell, type SigninShellProps } from '@/shell/signin/SigninShell';
import type { ChannelRow, VisibleStep } from '@/shell/signin/useSigninSteps';

const EMAIL_STEP: VisibleStep = { name: 'identifier', directory: null };
const PASSWORD_STEP: VisibleStep = { name: 'password', directory: null };
const DIRECTORY = {
  id: 'corp-ldap',
  name: 'Corp LDAP',
  kind: 'ldap',
  platform: 'custom',
} as const;

function stepProps(overrides: Partial<SigninShellProps['signin']> = {}) {
  return {
    step: EMAIL_STEP,
    rows: [],
    canGoBack: false,
    onBack: vi.fn(),
    onChoose: vi.fn(),
    passkeyBusy: false,
    passkeyAutofill: false,
    ...overrides,
  };
}

function channelList(rows: ChannelRow[], overrides: Partial<SigninShellProps['signin']> = {}) {
  return stepProps({ step: { name: 'channels' }, rows, ...overrides });
}

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
    signin: stepProps(),
    buildVersion: '26.0.0.0',
    ...overrides,
  };
}

type LinkProps = NonNullable<SigninShellProps['ssoLink']>;

function linkProps(overrides: Partial<LinkProps> = {}): LinkProps {
  return {
    link: {
      code: 'pending',
      providerName: 'Okta',
      organizationName: 'acme',
      emailHint: 'a***@example.com',
      emailAvailable: true,
      passwordAllowed: true,
    },
    method: 'password',
    onMethodChange: vi.fn(),
    emailCode: '',
    onEmailCodeChange: vi.fn(),
    password: '',
    onPasswordChange: vi.fn(),
    codeSentAt: null,
    sending: false,
    onResendCode: vi.fn(),
    ...overrides,
  };
}

function linkView(overrides: Partial<LinkProps> = {}) {
  return <SigninShell {...signinProps({ view: 'sso-link', ssoLink: linkProps(overrides) })} />;
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
    const { rerender } = render(<SigninShell {...signinProps()} />);

    expect(screen.getByRole('heading', { level: 1, name: 'Sign in' })).not.toBeNull();

    const looksShared = (input: HTMLElement) => {
      // 44px on a phone, 16px text so iOS does not zoom; 40px from `sm` up.
      expect(input.className).toContain('h-11');
      expect(input.className).toContain('sm:h-10');
      expect(input.className).toContain('text-[16px]');
      // The fill is the field's only edge.
      expect(input.className).toContain('bg-[var(--control-surface)]');
      expect(input.className).not.toMatch(/\bborder\b/);
    };

    looksShared(screen.getByLabelText('Email'));

    const next = screen.getByRole('button', { name: 'Continue →' }) as HTMLButtonElement;
    expect(next.disabled).toBe(true);
    expect(next.className).toContain('auth-primary-button');
    expect(next.className).toContain('sm:h-10');

    rerender(<SigninShell {...signinProps({ signin: stepProps({ step: PASSWORD_STEP }) })} />);
    looksShared(screen.getByLabelText('Password', { exact: true }));
    expect(
      (screen.getByRole('button', { name: 'Sign in →' }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it('lists the ways in, each with its mark and an arrow, and the email below them', () => {
    const onChoose = vi.fn();
    const github: ChannelRow = {
      kind: 'provider',
      provider: { id: 'gh', name: 'GitHub', kind: 'oauth2', platform: 'github' },
      lastUsed: false,
    };
    const corp: ChannelRow = {
      kind: 'provider',
      provider: { id: 'corp', name: 'Corp SAML', kind: 'saml', platform: 'custom' },
      lastUsed: false,
    };

    render(
      <SigninShell
        {...signinProps({ signin: channelList([github, corp], { onChoose }) })}
      />,
    );

    const list = screen.getByTestId('signin-channels');
    expect(within(list).getAllByRole('listitem')).toHaveLength(2);

    const row = screen.getByRole('button', { name: 'Continue with GitHub' });
    expect(row.querySelector('[data-sso-mark="github"]')).not.toBeNull();
    fireEvent.click(row);
    expect(onChoose).toHaveBeenCalledWith(github);

    // A generic provider shows its protocol's icon instead of a brand mark.
    const generic = screen.getByRole('button', { name: 'Continue with Corp SAML' });
    expect(generic.querySelector('[data-sso-mark]')).toBeNull();
    expect(generic.querySelector('svg')).not.toBeNull();

    // No field yet: one question at a time.
    expect(screen.queryByLabelText('Email')).toBeNull();
    expect(screen.queryByLabelText('Password', { exact: true })).toBeNull();
    expect(screen.getByRole('button', { name: 'Sign in with email →' })).not.toBeNull();
    expect(screen.getByRole('checkbox', { name: 'Remember me' })).not.toBeNull();
  });

  it('marks the way in used last time, and whose account it was', () => {
    render(
      <SigninShell
        {...signinProps({
          signin: channelList([
            { kind: 'passkey', lastUsed: false },
            {
              kind: 'provider',
              provider: { id: 'okta', name: 'Okta', kind: 'oidc', platform: 'okta' },
              lastUsed: true,
              account: 'alice@example.com',
            },
          ]),
        })}
      />,
    );

    const used = screen.getByRole('button', { name: /Continue with Okta/ });
    expect(used.textContent).toContain('Last used');
    expect(used.textContent).toContain('alice@example.com');
    expect(screen.getByTestId('signin-passkey').textContent).not.toContain('Last used');
  });

  it('offers the account that last signed in with its password as a way in of its own', () => {
    const row: ChannelRow = { kind: 'account', account: 'alice@example.com', lastUsed: true };
    const onChoose = vi.fn();

    render(<SigninShell {...signinProps({ signin: channelList([row], { onChoose }) })} />);

    const button = screen.getByRole('button', { name: /Continue with email/ });
    expect(button.textContent).toContain('alice@example.com');
    expect(button.textContent).toContain('Last used');
    fireEvent.click(button);
    expect(onChoose).toHaveBeenCalledWith(row);
  });

  it('asks for nothing while it cannot tell yet what there is to list', () => {
    render(<SigninShell {...signinProps({ signin: stepProps({ step: { name: 'waiting' } }) })} />);

    expect(screen.getByTestId('signin-waiting')).not.toBeNull();
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('asks for the password with the way back and the account above it', () => {
    const onBack = vi.fn();

    render(
      <SigninShell
        {...signinProps({
          email: 'alice@example.com',
          signin: stepProps({ step: PASSWORD_STEP, canGoBack: true, onBack }),
        })}
      />,
    );

    expect(screen.getByTestId('signin-account').textContent).toBe('alice@example.com');
    fireEvent.click(screen.getByRole('button', { name: 'Back' }));
    expect(onBack).toHaveBeenCalledOnce();

    expect(screen.getByRole('button', { name: 'Forgot password?' })).not.toBeNull();
    expect(screen.getByRole('checkbox', { name: 'Remember me' })).not.toBeNull();

    // A password manager saves the account with the password, from a field nobody sees.
    const account = document.querySelector<HTMLInputElement>(
      'input[autocomplete="username"]',
    );
    expect(account?.value).toBe('alice@example.com');
    expect(account?.readOnly).toBe(true);
    expect(account?.tabIndex).toBe(-1);
    expect(account?.className).toContain('sr-only');
    expect(screen.getByLabelText('Password', { exact: true }).getAttribute('autocomplete')).toBe(
      'current-password',
    );
  });

  it('shows the password on request and hides it again', () => {
    render(<SigninShell {...signinProps({ signin: stepProps({ step: PASSWORD_STEP }) })} />);

    const field = screen.getByLabelText('Password', { exact: true });
    expect(field.getAttribute('type')).toBe('password');

    fireEvent.click(screen.getByRole('button', { name: 'Show password' }));
    expect(field.getAttribute('type')).toBe('text');

    fireEvent.click(screen.getByRole('button', { name: 'Hide password' }));
    expect(field.getAttribute('type')).toBe('password');
  });

  it('asks a directory for its username, and leaves its password to it', () => {
    const { rerender } = render(
      <SigninShell
        {...signinProps({
          signin: stepProps({ step: { name: 'identifier', directory: DIRECTORY } }),
        })}
      />,
    );

    expect(screen.getByRole('heading', { level: 1, name: 'Sign in with Corp LDAP' })).not.toBeNull();
    const username = screen.getByLabelText('Username');
    expect(username.getAttribute('type')).toBe('text');
    expect(username.getAttribute('autocomplete')).toBe('username');

    rerender(
      <SigninShell
        {...signinProps({
          email: 'alice',
          signin: stepProps({ step: { name: 'password', directory: DIRECTORY } }),
        })}
      />,
    );

    expect(screen.getByText('Enter your Corp LDAP password.')).not.toBeNull();
    expect(screen.getByTestId('signin-account').textContent).toBe('alice');
    // The password is the directory's, not something this instance can reset.
    expect(screen.queryByRole('button', { name: 'Forgot password?' })).toBeNull();
  });

  it('says why a redirect sign-in that came back failed', () => {
    render(
      <SigninShell
        {...signinProps({
          ssoError: 'forbidden: email domain is not permitted to join this organization',
        })}
      />,
    );

    const alert = screen.getByRole('alert');
    expect(alert.textContent).toContain('email domain is not permitted');
  });

  it('shows placeholders that cannot be mistaken for typed values', () => {
    const { rerender } = render(<SigninShell {...signinProps()} />);

    expect(screen.getByLabelText('Email').getAttribute('placeholder')).toBe(
      'name@company.com',
    );

    rerender(<SigninShell {...signinProps({ signin: stepProps({ step: PASSWORD_STEP }) })} />);
    expect(screen.getByLabelText('Password', { exact: true }).getAttribute('placeholder')).toBe(
      'Enter your password',
    );
  });

  it('enables the primary action once the step has its answer', () => {
    const { rerender } = render(<SigninShell {...signinProps({ email: 'a@example.com' })} />);
    expect(
      (screen.getByRole('button', { name: 'Continue →' }) as HTMLButtonElement).disabled,
    ).toBe(false);

    rerender(
      <SigninShell
        {...signinProps({
          email: 'a@example.com',
          password: 'secret-pw',
          signin: stepProps({ step: PASSWORD_STEP }),
        })}
      />,
    );
    expect(
      (screen.getByRole('button', { name: 'Sign in →' }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });

  it('runs the pulse inside the submit button while the request is in flight', () => {
    render(
      <SigninShell
        {...signinProps({
          email: 'a@example.com',
          password: 'secret-pw',
          busy: true,
          signin: stepProps({ step: PASSWORD_STEP }),
        })}
      />,
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

  it('asks the owner to confirm a first single sign-on into an existing account', () => {
    const onMethodChange = vi.fn();
    const { rerender } = render(linkView({ onMethodChange }));

    expect(
      screen.getByRole('heading', { level: 1, name: 'Confirm it’s your account' }),
    ).not.toBeNull();
    expect(
      screen.getByText(/a\*\*\*@example\.com already has a MoleSignal account/).textContent,
    ).toContain('Okta (acme)');

    // The password is the quicker way, so it comes first.
    const methods = screen.getByRole('radiogroup', { name: 'Confirm with' });
    const radios = within(methods).getAllByRole('radio');
    expect(radios.map((radio) => radio.textContent)).toEqual(['Password', 'Email code']);
    expect(radios[0]?.getAttribute('aria-checked')).toBe('true');
    fireEvent.click(within(methods).getByRole('radio', { name: 'Email code' }));
    expect(onMethodChange).toHaveBeenCalledWith('email');

    expect(
      (screen.getByRole('button', { name: 'Confirm and sign in →' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    rerender(linkView({ password: 'secret-pw' }));
    expect(
      (screen.getByRole('button', { name: 'Confirm and sign in →' }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
  });

  it('sends the first code with the form button, then asks for its six digits', () => {
    const { rerender } = render(linkView({ method: 'email' }));

    const send = screen.getByRole('button', { name: 'Send code →' }) as HTMLButtonElement;
    expect(send.type).toBe('submit');
    expect(send.disabled).toBe(false);
    expect(screen.queryByLabelText('Verification code')).toBeNull();

    rerender(linkView({ method: 'email', codeSentAt: Date.now(), emailCode: '12345' }));
    const code = screen.getByLabelText('Verification code');
    expect(code.getAttribute('inputmode')).toBe('numeric');
    expect(code.getAttribute('autocomplete')).toBe('one-time-code');
    expect(
      (screen.getByRole('button', { name: 'Confirm and sign in →' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    // The server sends one code a minute; the resend button counts the wait down.
    expect(
      (screen.getByRole('button', { name: /^Resend in (60|59)s$/ }) as HTMLButtonElement).disabled,
    ).toBe(true);

    rerender(
      linkView({ method: 'email', codeSentAt: Date.now() - 61_000, emailCode: '123456' }),
    );
    expect(
      (screen.getByRole('button', { name: 'Confirm and sign in →' }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    expect((screen.getByRole('button', { name: 'Resend' }) as HTMLButtonElement).disabled).toBe(
      false,
    );
  });

  it('says so when the account cannot be confirmed here', () => {
    render(
      linkView({
        link: { ...linkProps().link, emailAvailable: false, passwordAllowed: false },
      }),
    );

    expect(screen.getByRole('alert').textContent).toContain('can’t be confirmed here');
    expect(screen.queryByRole('button', { name: /Confirm and sign in/ })).toBeNull();
    expect(screen.getByRole('button', { name: 'Back to sign in' })).not.toBeNull();
  });

  it('asks for the second step with an authenticator code or a recovery code', () => {
    const onCodeChange = vi.fn();
    const onMethodChange = vi.fn();
    const mfa = (method: 'totp' | 'recovery_code', code: string) =>
      signinProps({
        view: 'mfa',
        mfa: {
          methods: ['totp', 'recovery_code'],
          method,
          onMethodChange,
          code,
          onCodeChange,
          passkeyReady: false,
        },
      });

    const { rerender } = render(<SigninShell {...mfa('totp', '')} />);

    expect(screen.getByRole('heading', { name: 'Verify it’s you' })).toBeTruthy();
    expect(screen.getByText(/enter the 6-digit code for MoleSignal/)).toBeTruthy();

    // A pasted code keeps its six digits and nothing else.
    const field = screen.getByLabelText('Verification code');
    expect(field.getAttribute('autocomplete')).toBe('one-time-code');
    fireEvent.change(field, { target: { value: '12 34-56a7' } });
    expect(onCodeChange).toHaveBeenLastCalledWith('123456');
    expect(
      (screen.getByRole('button', { name: /Verify and sign in/ }) as HTMLButtonElement).disabled,
    ).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: 'Use a recovery code' }));
    expect(onCodeChange).toHaveBeenLastCalledWith('');
    expect(onMethodChange).toHaveBeenCalledWith('recovery_code');

    rerender(<SigninShell {...mfa('recovery_code', 'ABCDE-fghij')} />);
    expect(screen.getByText(/Enter a recovery code you haven’t used yet/)).toBeTruthy();
    expect(screen.getByLabelText('Recovery code')).toBeTruthy();
    expect(
      (screen.getByRole('button', { name: /Verify and sign in/ }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });

  it('asks for a passkey at the second step, once its ceremony has arrived', () => {
    const onMethodChange = vi.fn();
    const mfa = (passkeyReady: boolean) =>
      signinProps({
        view: 'mfa',
        mfa: {
          methods: ['totp', 'passkey', 'recovery_code'],
          method: 'passkey',
          onMethodChange,
          code: '',
          onCodeChange: vi.fn(),
          passkeyReady,
        },
      });

    const { rerender } = render(<SigninShell {...mfa(false)} />);

    expect(screen.getByText(/Verify it’s you with your passkey/)).toBeTruthy();
    expect(screen.queryByLabelText('Verification code')).toBeNull();
    expect(
      (screen.getByRole('button', { name: /Verify with a passkey/ }) as HTMLButtonElement).disabled,
    ).toBe(true);

    rerender(<SigninShell {...mfa(true)} />);
    expect(
      (screen.getByRole('button', { name: /Verify with a passkey/ }) as HTMLButtonElement).disabled,
    ).toBe(false);

    // The account's other ways stay one click away; there is no link to the current one.
    expect(screen.queryByRole('button', { name: 'Use a passkey' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Use the authenticator app' }));
    expect(onMethodChange).toHaveBeenCalledWith('totp');
    expect(screen.getByRole('button', { name: 'Use a recovery code' })).toBeTruthy();
  });

  it('offers a passkey on the list, and in the email field where the browser can suggest one', () => {
    const passkey: ChannelRow = { kind: 'passkey', lastUsed: false };
    const onChoose = vi.fn();
    const { rerender } = render(<SigninShell {...signinProps()} />);

    expect(screen.queryByTestId('signin-passkey')).toBeNull();
    expect(screen.getByLabelText('Email').getAttribute('autocomplete')).toBe('username');

    rerender(<SigninShell {...signinProps({ signin: stepProps({ passkeyAutofill: true }) })} />);
    expect(screen.getByLabelText('Email').getAttribute('autocomplete')).toBe('username webauthn');

    rerender(<SigninShell {...signinProps({ signin: channelList([passkey], { onChoose }) })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Continue with a passkey' }));
    expect(onChoose).toHaveBeenCalledWith(passkey);

    rerender(
      <SigninShell
        {...signinProps({ signin: channelList([passkey], { onChoose, passkeyBusy: true }) })}
      />,
    );
    expect(
      (screen.getByRole('button', { name: 'Waiting for the passkey…' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });
});
