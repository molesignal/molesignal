import './shell/tokens.css';
import './shell/polish/index.css';
import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import React from 'react';
import ReactDOM from 'react-dom/client';
import { RouterProvider } from 'react-router-dom';

import { KeyboardProvider } from '@/keyboard/controller';
import { router } from '@/routes';
import { OrgEntryDialog } from '@/routes/account/mfa/OrgEntryDialog';
import { ThemeBootstrap } from '@/shell/ThemeBootstrap';
import { Toaster } from '@/shell/ui/sonner';
import { TooltipProvider } from '@/shell/ui/tooltip';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ThemeBootstrap>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={200}>
          <KeyboardProvider>
            <RouterProvider router={router} future={{ v7_startTransition: true }} />
            <OrgEntryDialog onOpenPath={(path) => void router.navigate(path)} />
            <Toaster />
          </KeyboardProvider>
        </TooltipProvider>
      </QueryClientProvider>
    </ThemeBootstrap>
  </React.StrictMode>,
);
