import { ListChecks } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useProductAccess } from '@/product/access';
import { ChromeButton } from '@/shell/chrome';

import { canUseSearchJobs } from './access';

/**
 * The way from the stream list to the organization's search jobs. It is there
 * only for those who may see the jobs: a way to a page that turns one away is
 * not a way.
 */
export function SearchJobsEntry() {
  const { t } = useTranslation('streams');
  const navigate = useNavigate();
  const access = useProductAccess();
  if (!canUseSearchJobs(access)) return null;

  return (
    <ChromeButton onClick={() => navigate('/streams/jobs')}>
      <ListChecks className="h-3 w-3" /> {t('jobs.entry')}
    </ChromeButton>
  );
}
