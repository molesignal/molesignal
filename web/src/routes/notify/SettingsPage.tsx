import type { ReactNode } from 'react';

import { ProductState, type ProductStateProps } from '@/product/states';
import { PageHeader } from '@/shell/PageHeader';

import { SectionBody } from '../settings/_atoms';

export function NotifySettingsPage({
  title,
  subtitle,
  toolbar,
  filters,
  state,
  children,
}: {
  title: ReactNode;
  subtitle?: string;
  toolbar?: ReactNode;
  filters?: ReactNode;
  state?: ProductStateProps | null;
  children?: ReactNode;
}) {
  return (
    <>
      <PageHeader title={title} subtitle={subtitle} toolbar={toolbar} />
      <SectionBody className="space-y-4 pb-10">
        {filters}
        {state ? <ProductState {...state} /> : children}
      </SectionBody>
    </>
  );
}
