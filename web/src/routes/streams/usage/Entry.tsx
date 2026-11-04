import { Gauge } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { canAccessProductPath, useProductAccess } from '@/product/access';
import { ChromeButton } from '@/shell/chrome';

/** The way from the stream list to what each source takes in, for those who may see it. */
export function IntakeUsageEntry() {
  const { t } = useTranslation('streams');
  const navigate = useNavigate();
  const access = useProductAccess();
  if (!canAccessProductPath('/streams/usage', access)) return null;

  return (
    <ChromeButton onClick={() => navigate('/streams/usage')}>
      <Gauge className="h-3 w-3" /> {t('usage.entry')}
    </ChromeButton>
  );
}
