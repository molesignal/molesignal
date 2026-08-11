import { useTranslation } from 'react-i18next';

import { useStatusPageWorkspace } from '../Layout';
import { CustomDomainSettings } from './CustomDomain';
import { PrivateAccessSettings } from './PrivateAccess';

export function StatusPageDomainAccessSettings() {
  const { t } = useTranslation('status-pages');
  const { snapshot } = useStatusPageWorkspace();

  return (
    <div className="space-y-5">
      <CustomDomainSettings />
      {snapshot.page.visibility === 'private' ? (
        <PrivateAccessSettings />
      ) : (
        <div className="rounded-lg border border-bd-0 bg-bg-1 px-5 py-8 text-center">
          <h2 className="text-sm font-display-strong text-tx-0">{t('settings.private_access.public_title')}</h2>
          <p className="mx-auto mt-1 max-w-xl text-xs leading-5 text-tx-3">
            {t('settings.private_access.public_description')}
          </p>
        </div>
      )}
    </div>
  );
}
