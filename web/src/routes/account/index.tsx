import { useTranslation } from 'react-i18next';

import {
  type FeatureKey,
  useFeatureGate,
} from '@/product/edition';
import {
  EditionGatePage,
  useFeatureGateCopy,
} from '@/product/FeatureGate';
import { GatePage } from '@/product/templates';

export function AccountBilling() {
  return <AccountFeaturePage feature="saas-billing" />;
}

export function AccountSupport() {
  return <AccountFeaturePage feature="saas-support" />;
}

function AccountFeaturePage({ feature }: { feature: FeatureKey }) {
  const { t } = useTranslation('edition');
  const gate = useFeatureGate(feature);
  const copy = useFeatureGateCopy(gate);
  const title = t(gate.feature.labelKey);
  if (gate.status !== 'allowed') {
    return <EditionGatePage gate={gate} title={title} />;
  }
  return (
    <GatePage
      title={title}
      breadcrumbs={null}
      backTo={null}
      state={{
        variant: 'empty',
        title: copy.title,
        description: copy.description,
      }}
    />
  );
}
