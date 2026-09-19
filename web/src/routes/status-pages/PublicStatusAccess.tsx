import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CheckCircle2, LockKeyhole, Mail } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';

import { resolveStatusPageLanguage } from './model';
import type { PublicStatusSource } from './publicStatusRouting';

export function PublicStatusAccess({
  source,
  slug,
}: {
  source: PublicStatusSource;
  slug: string;
}) {
  const { t, i18n } = useTranslation('status-pages');
  const queryClient = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const [email, setEmail] = React.useState('');
  const [requested, setRequested] = React.useState(false);
  const accessToken = searchParams.get('access_token');
  const metadata = useQuery({
    queryKey: ['public-status-page-access', source, source === 'slug' ? slug : 'current'],
    queryFn: () =>
      source === 'domain'
        ? statusPagesApi.getAccessMetadataByDomain()
        : statusPagesApi.getAccessMetadata(slug),
  });
  const consume = useMutation({
    mutationFn: (token: string) =>
      source === 'domain'
        ? statusPagesApi.consumePrivateAccessByDomain(token)
        : statusPagesApi.consumePrivateAccess(slug, token),
    onSuccess: async () => {
      const next = new URLSearchParams(searchParams);
      next.delete('access_token');
      setSearchParams(next, { replace: true });
      await queryClient.invalidateQueries({ queryKey: ['public-status-page', source] });
    },
  });
  const request = useMutation({
    mutationFn: () =>
      source === 'domain'
        ? statusPagesApi.requestPrivateAccessByDomain(email.trim())
        : statusPagesApi.requestPrivateAccess(slug, email.trim()),
    onSuccess: () => setRequested(true),
  });
  const consumedRef = React.useRef<string | null>(null);
  React.useEffect(() => {
    if (!accessToken || consumedRef.current === accessToken) return;
    consumedRef.current = accessToken;
    consume.mutate(accessToken);
  }, [accessToken, consume]);

  if (metadata.isLoading || (accessToken && consume.isPending)) {
    return <AccessFrame><p className="text-sm text-tx-2">{t('states.loading')}</p></AccessFrame>;
  }
  if (!metadata.data) {
    return (
      <AccessFrame>
        <p className="text-sm text-red-soft">{toApiError(metadata.error).message}</p>
      </AccessFrame>
    );
  }
  const page = metadata.data;
  const language = resolveStatusPageLanguage(page.language, page.languages, searchParams.get('lang'));
  const copy = i18n.getFixedT(language, 'status-pages');
  const consumeError = consume.error ? toApiError(consume.error).message : null;
  const requestError = request.error ? toApiError(request.error).message : null;

  return (
    <AccessFrame brandColor={page.brand_color}>
      <div className="mx-auto w-full max-w-md rounded-2xl border border-bd-0 bg-white p-6 sm:p-8">
        <div className="flex items-center gap-3">
          {page.logo_url ? (
            <img src={page.logo_url} alt="" className="h-12 w-12 rounded-lg border border-bd-0 object-contain p-1" />
          ) : (
            <span
              aria-hidden
              className="grid h-12 w-12 place-items-center rounded-lg text-lg font-bold text-white"
              style={{ backgroundColor: page.brand_color }}
            >
              {page.name.slice(0, 1).toUpperCase()}
            </span>
          )}
          <div className="min-w-0">
            <h1 className="truncate text-xl font-display-strong text-tx-0">{page.name}</h1>
            <p className="mt-0.5 text-sm text-tx-2">{copy('public.access.private_status_page')}</p>
          </div>
        </div>

        {consumeError ? (
          <div className="mt-7 rounded-lg bg-red-dim p-4">
            <p className="text-sm font-strong text-red-soft">{copy('public.access.invalid_link')}</p>
            <p className="mt-1 text-xs leading-5 text-red-soft/80">{consumeError}</p>
          </div>
        ) : requested ? (
          <div className="mt-7 rounded-lg bg-green-dim p-4">
            <div className="flex items-center gap-2 text-sm font-strong text-green-soft">
              <CheckCircle2 className="h-4 w-4" />
              {copy('public.access.check_email')}
            </div>
            <p className="mt-2 text-xs leading-5 text-tx-2">{copy('public.access.requested')}</p>
          </div>
        ) : (
          <form
            className="mt-7 space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              if (email.trim()) request.mutate();
            }}
          >
            <div className="rounded-lg bg-bg-1 p-4">
              <div className="flex items-center gap-2 text-sm font-strong text-tx-0">
                <LockKeyhole className="h-4 w-4 text-tx-2" />
                {copy('public.access.title')}
              </div>
              <p className="mt-2 text-xs leading-5 text-tx-2">{copy('public.access.description')}</p>
            </div>
            <label className="block text-sm font-strong text-tx-1">
              <span>{copy('public.access.email')}</span>
              <div className="relative mt-2">
                <Mail className="pointer-events-none absolute left-3 top-3 h-4 w-4 text-tx-3" />
                <Input
                  type="email"
                  autoComplete="email"
                  required
                  value={email}
                  onChange={(event) => setEmail(event.currentTarget.value)}
                  placeholder="you@example.com"
                  className="h-11 bg-white pl-9 text-base sm:text-sm"
                />
              </div>
            </label>
            {requestError && <p role="alert" className="text-xs text-red-soft">{requestError}</p>}
            <Button
              type="submit"
              size="lg"
              disabled={!email.trim() || request.isPending}
              className="h-11 w-full text-white"
              style={{ backgroundColor: page.brand_color }}
            >
              {request.isPending ? copy('public.access.sending') : copy('public.access.send_link')}
            </Button>
          </form>
        )}
      </div>
    </AccessFrame>
  );
}

function AccessFrame({
  brandColor = '#4F46E5',
  children,
}: {
  brandColor?: string;
  children: React.ReactNode;
}) {
  return (
    <main data-theme="light" className="min-h-screen overflow-y-auto bg-bg-0 text-tx-1">
      <div className="h-1 w-full" style={{ backgroundColor: brandColor }} />
      <div className="grid min-h-[calc(100vh-4px)] place-items-center px-4 py-10">{children}</div>
    </main>
  );
}
