import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { codeComplete } from '@/routes/account/mfa/CodeField';
import { ProofDialog } from '@/routes/account/mfa/ProofDialog';

vi.mock('@/shell/ui/sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

vi.mock('@/api/mfa', () => ({
  passkeyProof: vi.fn(() =>
    Promise.resolve({
      ceremony: 'mock-ceremony',
      options: {
        publicKey: {
          challenge: 'AQID',
          rpId: 'localhost',
          allowCredentials: [],
          userVerification: 'required',
        },
      },
    }),
  ),
}));

function dialog(overrides: Partial<React.ComponentProps<typeof ProofDialog>> = {}) {
  return (
    <ProofDialog
      open
      onOpenChange={vi.fn()}
      methods={['totp', 'recovery_code']}
      title="Turn off multi-factor authentication?"
      description="Confirm with a code."
      confirmLabel="Turn off"
      busyLabel="Turning off…"
      busy={false}
      failures={0}
      onConfirm={vi.fn()}
      {...overrides}
    />
  );
}

describe.sequential('ProofDialog', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
  });

  afterEach(() => {
    cleanup();
  });

  it('knows a whole code from a partial one', () => {
    expect(codeComplete('totp', '12345')).toBe(false);
    expect(codeComplete('totp', '123456')).toBe(true);
    expect(codeComplete('recovery_code', 'abcde')).toBe(false);
    expect(codeComplete('recovery_code', 'ABCDE-fghij')).toBe(true);
    expect(codeComplete('recovery_code', 'abcde fghij')).toBe(true);
  });

  it('confirms only with a whole code, from the app or a recovery code', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(dialog({ onConfirm }));

    // The button is mounted anew when it turns enabled: look it up again each time.
    const confirm = () => screen.getByRole('button', { name: 'Turn off' });
    expect(confirm()).toHaveProperty('disabled', true);

    // Only digits reach the field, and at most six of them.
    await user.type(screen.getByLabelText('Verification code'), '12a34-5678');
    expect(screen.getByLabelText('Verification code')).toHaveProperty('value', '123456');
    await user.click(confirm());
    expect(onConfirm).toHaveBeenLastCalledWith({ method: 'totp', code: '123456' });

    await user.click(screen.getByRole('button', { name: 'Use a recovery code' }));
    const recovery = screen.getByLabelText('Recovery code');
    expect(recovery).toHaveProperty('value', '');
    await user.type(recovery, 'abcde-fghij');
    await user.click(confirm());
    expect(onConfirm).toHaveBeenLastCalledWith({ method: 'recovery_code', code: 'abcde-fghij' });
  });

  it('empties the code after a failed try', async () => {
    const user = userEvent.setup();
    const { rerender } = render(dialog());

    await user.type(screen.getByLabelText('Verification code'), '123456');
    rerender(dialog({ failures: 1 }));

    expect(screen.getByLabelText('Verification code')).toHaveProperty('value', '');
  });

  it('asks for the password when the account has no second factor yet', async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(dialog({ methods: [], password: true, onConfirm }));

    const confirm = () => screen.getByRole('button', { name: 'Turn off' });
    expect(confirm()).toHaveProperty('disabled', true);
    expect(screen.queryByRole('button', { name: 'Use a recovery code' })).toBeNull();

    await user.type(screen.getByLabelText('Account password'), 'hunter2');
    await user.click(confirm());
    expect(onConfirm).toHaveBeenLastCalledWith({ method: 'password', password: 'hunter2' });
  });

  it('offers a passkey first, and waits for its ceremony before the button works', async () => {
    const user = userEvent.setup();
    render(dialog({ methods: ['passkey', 'totp', 'recovery_code'] }));

    // Nothing to type: the button asks the browser, once the ceremony has arrived.
    expect(screen.queryByLabelText('Verification code')).toBeNull();
    expect(screen.getByRole('button', { name: 'Turn off' })).toHaveProperty('disabled', true);
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Turn off' })).toHaveProperty('disabled', false),
    );

    await user.click(screen.getByRole('button', { name: 'Use the authenticator app' }));
    expect(screen.getByLabelText('Verification code')).toHaveProperty('value', '');
    expect(screen.getByRole('button', { name: 'Use a passkey' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Use a recovery code' })).toBeTruthy();
  });
});
