import { useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import * as ssoApi from '@/api/sso';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { IamListPage } from '@/routes/iam/IamLayout';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';

import { DirectoryTestDialog } from './DirectoryTestDialog';
import { testResultPath } from './model';
import { Findings, ResultNotes, ResultVerdict } from './ResultParts';
import { useSigninTestRun } from './useSigninTestRun';
import { formatMicros } from '../../../rum/_helpers';

/** How often a page waiting for the identity provider asks again. */
const POLL_MS = 2_000;

/**
 * What a test sign-in found: whether it went through, what the identity provider
 * returned, and what a real sign-in would do with it.
 */
export function SsoSigninTestResult() {
  const { t } = useTranslation('settings-admin');
  const { id = '' } = useParams();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'org.sso.manage' });
  const run = useSigninTestRun({ onFinished: (testId) => navigate(testResultPath(testId)) });

  const query = useQuery({
    queryKey: ['sso', 'test', id],
    queryFn: () => ssoApi.getSigninTest(id),
    refetchInterval: (current) => (current.state.data?.status === 'pending' ? POLL_MS : false),
    refetchOnWindowFocus: true,
  });

  const test = query.data;
  const status = test?.status;

  // The list's words are out of date the moment a result lands.
  React.useEffect(() => {
    if (status && status !== 'pending') {
      void queryClient.invalidateQueries({ queryKey: ['sso', 'providers'] });
    }
  }, [queryClient, status]);

  const pageState = productStateFor(
    queryStateFor({ isLoading: query.isLoading, isError: query.isError, data: test }),
    { error: query.error },
  );

  const testAgain = () => {
    if (test) run.begin({ id: test.providerId, name: test.providerName, kind: test.kind });
  };

  return (
    <>
      {test && (
        <DirectoryTestDialog
          providerName={test.providerName}
          open={run.directory !== null}
          busy={run.testingDirectory}
          onClose={run.closeDirectory}
          onSubmit={run.submitDirectory}
        />
      )}
      <IamListPage
        title={
          test
            ? t('sso_providers.test.result.title', { name: test.providerName })
            : t('sso_providers.test.title')
        }
        subtitle={
          test
            ? t(
                test.startedByYou
                  ? 'sso_providers.test.result.meta_you'
                  : 'sso_providers.test.result.meta',
                { who: test.startedBy, when: formatMicros(test.startedAt) },
              )
            : undefined
        }
        toolbar={
          <>
            <ChromeButton onClick={() => navigate('/iam/sso')}>
              {t('sso_providers.test.result.back')}
            </ChromeButton>
            {test && (
              <ChromeButton
                variant="primary"
                disabled={manageAccess.disabled || run.starting || run.waitingFor !== null}
                disabledReason={manageAccess.reason}
                onClick={testAgain}
                data-testid="sso-test-again"
              >
                {run.waitingFor
                  ? t('sso_providers.test.standing.waiting')
                  : t('sso_providers.test.result.again')}
              </ChromeButton>
            )}
          </>
        }
        state={pageState}
      >
        {test && (
          <div className="flex min-w-0 max-w-[872px] flex-col gap-5 pb-2" data-testid="sso-test-result">
            <ResultVerdict test={test} />
            <ResultNotes test={test} />
            <Findings test={test} />
          </div>
        )}
      </IamListPage>
    </>
  );
}
