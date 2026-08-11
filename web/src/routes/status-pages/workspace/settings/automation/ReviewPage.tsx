import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { PageBody } from '@/shell/PageHeader';

import { CandidateReviewDrawer } from './CandidateReviewDrawer';
import { useStatusPageWorkspace } from '../../Layout';

export function StatusPageAutomationReview() {
  const { t } = useTranslation('status-pages');
  const navigate = useNavigate();
  const { candidateId } = useParams();
  const { pageId, snapshot } = useStatusPageWorkspace();
  const publishAccess = useActionAccess({ permission: 'status_pages.publish' });

  return (
    <PageBody>
      <div className="rounded-lg border border-bd-0 bg-bg-1 px-5 py-8 text-center">
        <div className="text-sm font-strong text-tx-0">{t('automation.review.title')}</div>
        <div className="mt-1 text-xs text-tx-3">{t('automation.review.subtitle')}</div>
      </div>
      <CandidateReviewDrawer
        pageId={pageId}
        candidateId={candidateId ?? null}
        components={snapshot.components}
        canPublish={publishAccess.allowed}
        publishDisabledReason={publishAccess.reason}
        onClose={() => navigate(`/status-pages/${pageId}/incidents`, { replace: true })}
      />
    </PageBody>
  );
}
