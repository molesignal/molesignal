import type { ReactNode } from 'react';

import { listContent } from '@/product/listContent';
import { ProductState, type ProductStateProps } from '@/product/states';
import { PageTitleRow } from '@/shell/PageTitleRow';

import { SectionBody } from '../settings/_atoms';

export function NotifySettingsPage({
  title,
  subtitle,
  toolbar,
  filters,
  state: requestedState,
  children: list,
}: {
  title: ReactNode;
  subtitle?: string;
  toolbar?: ReactNode;
  filters?: ReactNode;
  state?: ProductStateProps | null;
  children?: ReactNode;
}) {
  const { state, children } = listContent(requestedState, list);

  return (
    <div data-notify-settings-page className="min-w-0">
      <header data-notify-settings-header className="min-w-0">
        <PageTitleRow
          title={title}
          description={subtitle}
          actions={toolbar}
          level={2}
          size="section"
        />
      </header>
      <SectionBody className="space-y-5 pb-10">
        {filters}
        {state ? <ProductState {...state} /> : children}
      </SectionBody>
    </div>
  );
}
