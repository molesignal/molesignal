import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as ssoApi from '@/api/sso';
import { toApiError } from '@/lib/http';
import { toast } from '@/shell/ui/sonner';

import { testResultPath } from './model';

const PROVIDERS_KEY = ['sso', 'providers'] as const;

/** How often the page asks whether the other tab has finished. */
const POLL_MS = 2_000;

export type TestableProvider = Pick<ssoApi.SsoProvider, 'id' | 'name' | 'kind'>;

/** Sends the opened tab to the identity provider, or this one when no tab could be opened. */
function goToIdentityProvider(tab: Window | null, url: string): void {
  if (tab) tab.location.href = url;
  else window.location.assign(url);
}

/**
 * Starts a test sign-in of a saved provider. A redirect sign-in happens in a tab of its
 * own: an identity provider that shows an error page never comes back, and whoever is
 * testing shouldn't be left there with the settings gone. This page follows the test
 * until it has a result. A directory is asked for an account first.
 */
export function useSigninTestRun({
  onFinished,
}: {
  /** The test out in another tab has a result. */
  onFinished?: (testId: string) => void;
} = {}) {
  const { t } = useTranslation('settings-admin');
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [waiting, setWaiting] = React.useState<{
    provider: TestableProvider;
    testId: string;
    tab: Window | null;
  } | null>(null);
  const [directory, setDirectory] = React.useState<TestableProvider | null>(null);

  const startMutation = useMutation({
    mutationFn: ({ provider }: { provider: TestableProvider; tab: Window | null }) =>
      ssoApi.startSigninTest(provider.id),
    onSuccess: ({ id, authorizationUrl: authorization_url }, { provider, tab }) => {
      goToIdentityProvider(tab, authorization_url);
      setWaiting({ provider, testId: id, tab });
    },
    onError: (error, { tab }) => {
      tab?.close();
      toast.error(toApiError(error).message);
    },
  });

  const directoryMutation = useMutation({
    mutationFn: ({ providerId, username, password }: {
      providerId: string;
      username: string;
      password: string;
    }) => ssoApi.testDirectorySignin(providerId, { username, password }),
    onSuccess: (test) => {
      setDirectory(null);
      void queryClient.invalidateQueries({ queryKey: PROVIDERS_KEY });
      navigate(testResultPath(test.id));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const followed = useQuery({
    queryKey: ['sso', 'test', waiting?.testId],
    queryFn: () => ssoApi.getSigninTest(waiting?.testId ?? ''),
    enabled: waiting !== null,
    refetchInterval: POLL_MS,
    refetchOnWindowFocus: true,
    retry: false,
  });

  const followedStatus = followed.data?.status;
  const followedId = followed.data?.id;
  const followingFailed = followed.isError;

  React.useEffect(() => {
    if (followingFailed) setWaiting(null);
  }, [followingFailed]);

  // Whoever closed the tab isn't coming back from it: nothing is left to wait for, and the list
  // reads whatever result there is.
  React.useEffect(() => {
    const tab = waiting?.tab;
    if (!tab) return undefined;

    const check = window.setInterval(() => {
      if (!tab.closed) return;

      setWaiting(null);
      void queryClient.invalidateQueries({ queryKey: PROVIDERS_KEY });
    }, POLL_MS);

    return () => window.clearInterval(check);
  }, [queryClient, waiting]);

  React.useEffect(() => {
    if (!waiting || followedId !== waiting.testId) return;
    if (!followedStatus || followedStatus === 'pending') return;

    const { name } = waiting.provider;
    setWaiting(null);
    void queryClient.invalidateQueries({ queryKey: PROVIDERS_KEY });
    onFinished?.(followedId);

    if (followedStatus === 'passed') toast.success(t('sso_providers.test.toast_passed', { name }));
    else if (followedStatus === 'failed') toast.error(t('sso_providers.test.toast_failed', { name }));
    else toast.error(t('sso_providers.test.toast_expired', { name }));
  }, [followedId, followedStatus, onFinished, queryClient, t, waiting]);

  const begin = (provider: TestableProvider) => {
    if (provider.kind === 'ldap') {
      setDirectory(provider);
      return;
    }

    // Opened in the click itself, or the browser takes it for a popup. It has no way back here.
    const tab = window.open('', '_blank');
    if (tab) tab.opener = null;

    startMutation.mutate({ provider, tab });
  };

  return {
    begin,
    /** The provider whose test is out in another tab. */
    waitingFor: waiting?.provider.id ?? null,
    starting: startMutation.isPending,
    directory,
    closeDirectory: () => setDirectory(null),
    testingDirectory: directoryMutation.isPending,
    submitDirectory: (username: string, password: string) => {
      if (directory) {
        directoryMutation.mutate({ providerId: directory.id, username, password });
      }
    },
  };
}
