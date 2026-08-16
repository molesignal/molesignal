import type { ReactNode } from 'react';

import { ProductState, type ProductStateProps } from '@/product/states';

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
    <div data-notify-settings-page className="min-w-0">
      <header
        data-notify-settings-header
        className="flex min-w-0 flex-wrap items-start gap-4 pb-1"
      >
        <div className="min-w-[240px] flex-1">
          <h2 className="type-section-title font-sans font-display-strong text-tx-0">
            {title}
          </h2>
          {subtitle && (
            <p className="mt-1 max-w-3xl text-sm leading-relaxed text-tx-2">
              {subtitle}
            </p>
          )}
        </div>
        {toolbar && (
          <div className="ml-auto flex max-w-full flex-wrap items-center justify-end gap-2">
            {toolbar}
          </div>
        )}
      </header>
      <SectionBody className="space-y-5 pb-10 pt-5">
        {filters}
        {state ? <ProductState {...state} /> : children}
      </SectionBody>
    </div>
  );
}
