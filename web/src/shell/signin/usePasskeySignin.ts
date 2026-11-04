import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as auth from '@/api/auth';
import { toApiError } from '@/lib/http';
import {
  forgetPasskey,
  getPasskey,
  passkeyAutofillSupported,
  passkeyFailure,
  type PasskeyAssertion,
  type PasskeyCeremony,
  type PasskeyRequestOptions,
} from '@/lib/webauthn';
import { toast } from '@/shell/ui/sonner';

/** A ceremony lives five minutes on the server; a fresh one is fetched before that. */
const REFRESH_MS = 4 * 60_000;
/** After the browser ended the autofill request by itself. */
const RETRY_MS = 10_000;

type Prepared = PasskeyCeremony<PasskeyRequestOptions>;

/**
 * Signing in with a passkey without typing anything. While `enabled`, a ceremony is at hand for
 * the button: only a click may start the browser's prompt on Safari, so the button can't wait for
 * a request first. While `autofill` too, the same ceremony waits behind the username field for the
 * browser's suggestions: the field has to be on the page by then, so that is only while it is.
 */
export function usePasskeySignin({
  enabled,
  autofill,
  remember,
  onResult,
  onSsoOnly,
}: {
  enabled: boolean;
  autofill: boolean;
  remember: boolean;
  onResult: (result: auth.SigninResult, remember: boolean) => Promise<void>;
  /** The workspace only lets single sign-on in; the message points at its providers. */
  onSsoOnly: () => void;
}) {
  const { t } = useTranslation(['errors', 'shell']);
  const [busy, setBusy] = React.useState(false);
  const [rearm, setRearm] = React.useState(0);
  const waiting = React.useRef<{
    prepared: Promise<Prepared>;
    /** The same ceremony once it has arrived. */
    ready: Prepared | null;
    stop: AbortController;
  } | null>(null);
  const latest = React.useRef({ remember, onResult, onSsoOnly });
  latest.current = { remember, onResult, onSsoOnly };

  const failed = (error: unknown) => {
    const failure = passkeyFailure(error);

    if (failure === 'cancelled') return;

    const apiError = toApiError(error);
    const ssoOnly =
      apiError.status === 403 && apiError.message.includes('only allows single sign-on');

    toast.error(t('errors:sign_in_failed'), {
      description:
        failure === 'wrong_address'
          ? t('shell:signin.passkey_wrong_address')
          : ssoOnly
            ? t('shell:signin.sso_only')
            : apiError.message,
    });

    if (ssoOnly) latest.current.onSsoOnly();
  };

  const finish = async (prepared: Prepared, credential: PasskeyAssertion) => {
    setBusy(true);

    try {
      const { remember: keep, onResult: settle } = latest.current;
      await settle(
        await auth.signinWithPasskey({
          ceremony: prepared.ceremony,
          credential,
          remember: keep,
        }),
        keep,
      );
      // Signed in: the page is on its way out, so nothing waits behind the field again.
    } catch (error) {
      const apiError = toApiError(error);

      if (apiError.status === 401 && apiError.message.includes('not registered here')) {
        // Removed from the account, still kept by the provider: it should stop offering it.
        forgetPasskey(prepared.options.publicKey.rpId, credential.id);
        toast.error(t('errors:sign_in_failed'), {
          description: t('shell:signin.passkey_unknown'),
        });
      } else {
        failed(error);
      }

      setBusy(false);
    }
  };

  React.useEffect(() => {
    if (!enabled || busy) return undefined;

    const stop = new AbortController();
    const prepared = auth.passkeySigninOptions();
    let timer = window.setTimeout(() => setRearm((count) => count + 1), REFRESH_MS);
    const current = { prepared, ready: null as Prepared | null, stop };
    waiting.current = current;

    const again = (delay: number) => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setRearm((count) => count + 1), delay);
    };

    const wait = async () => {
      const ceremony = await prepared;
      current.ready = ceremony;

      if (stop.signal.aborted || !autofill || !(await passkeyAutofillSupported())) return;

      const credential = await getPasskey(ceremony.options, {
        autofill: true,
        signal: stop.signal,
      });

      if (!stop.signal.aborted) void finish(ceremony, credential);
    };

    wait().catch((error: unknown) => {
      // Nothing is said about a request the person didn't start; the button still works.
      // Only the browser ending the request is worth trying again soon: anything else
      // (the wrong address, the server saying no) would fail the same way.
      if (!stop.signal.aborted && passkeyFailure(error) === 'cancelled') again(RETRY_MS);
    });

    return () => {
      stop.abort();
      window.clearTimeout(timer);
      waiting.current = null;
    };
    // `finish` reads the latest values through `latest`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, autofill, busy, rearm]);

  const signIn = async () => {
    const current = waiting.current;

    // One browser request at a time: the autofill one makes way for the button's.
    current?.stop.abort();
    setBusy(true);

    try {
      // With the ceremony at hand the browser is asked within the click.
      const prepared =
        current?.ready ?? (await (current?.prepared ?? auth.passkeySigninOptions()));
      const credential = await getPasskey(prepared.options);
      await finish(prepared, credential);
    } catch (error) {
      failed(error);
      setBusy(false);
    }
  };

  return { signIn, busy };
}
