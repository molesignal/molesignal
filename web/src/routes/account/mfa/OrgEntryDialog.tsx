import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MfaMethod } from '@/api/auth';
import type { Proof } from '@/api/mfa';
import * as orgsApi from '@/api/orgs';
import * as ssoApi from '@/api/sso';
import { ChromeButton } from '@/shell/chrome';
import { keepSsoTrip } from '@/shell/signin/lastSignin';
import { preferredMethod } from '@/shell/signin/MfaPanel';
import { SsoPlatformMark } from '@/shell/SsoPlatformMark';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { useOrgEntry } from '@/stores/useOrgEntry';

import { useCodeErrorToast } from './CodeField';
import { ProofDialog } from './ProofDialog';

/** The account's second factors, the one the second step would offer first coming first. */
function inPreferredOrder(methods: MfaMethod[]): MfaMethod[] {
  const first = preferredMethod(methods);

  return [first, ...methods.filter((method) => method !== first)];
}

function ProofEntry({
  orgId,
  organizationName,
  methods,
  settle,
}: {
  orgId: string;
  organizationName: string;
  methods: MfaMethod[];
  settle: (entered: orgsApi.SelectOrgResponse | null) => void;
}) {
  const { t } = useTranslation('account');
  const failed = useCodeErrorToast();
  const [failures, setFailures] = React.useState(0);

  const enter = useMutation({
    mutationFn: (proof: Proof) => orgsApi.selectOrg(orgId, proof),
    onSuccess: (result) => settle('orgId' in result ? result : null),
    onError: (error) => {
      setFailures((count) => count + 1);
      failed(error);
    },
  });

  return (
    <ProofDialog
      testId="org-entry-proof"
      open
      onOpenChange={(open) => !open && settle(null)}
      methods={inPreferredOrder(methods)}
      title={t('security.org_entry.mfa_title', { organization: organizationName })}
      description={t('security.org_entry.mfa_description')}
      confirmLabel={t('security.org_entry.mfa_confirm')}
      busyLabel={t('security.mfa.checking')}
      busy={enter.isPending}
      failures={failures}
      onConfirm={(proof) => enter.mutate(proof)}
    />
  );
}

/**
 * Asks for what entering an organization still needs, for whichever page switched
 * organization: a second factor on the spot, setting one up first, or signing in
 * through the organization's own single sign-on.
 */
export function OrgEntryDialog({ onOpenPath }: { onOpenPath: (path: string) => void }) {
  const { t } = useTranslation(['account', 'common']);
  const asking = useOrgEntry((state) => state.asking);

  if (!asking) return null;

  const { orgId, waiting, settle } = asking;

  if ('mfa' in waiting) {
    return (
      <ProofEntry
        orgId={orgId}
        organizationName={waiting.mfa.organizationName}
        methods={waiting.mfa.methods}
        settle={settle}
      />
    );
  }

  const close = (open: boolean) => {
    if (!open) settle(null);
  };

  if ('mfaSetup' in waiting) {
    return (
      <Dialog open onOpenChange={close}>
        <DialogContent data-testid="org-entry-setup">
          <DialogHeader>
            <DialogTitle>
              {t('security.org_entry.setup_title', {
                organization: waiting.mfaSetup.organizationName,
              })}
            </DialogTitle>
            <DialogDescription>{t('security.org_entry.setup_description')}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <ChromeButton onClick={() => settle(null)}>{t('common:actions.cancel')}</ChromeButton>
            <ChromeButton
              variant="primary"
              onClick={() => {
                settle(null);
                onOpenPath('/account/settings/security');
              }}
            >
              {t('security.org_entry.setup_action')}
            </ChromeButton>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  return (
    <Dialog open onOpenChange={close}>
      <DialogContent data-testid="org-entry-sso">
        <DialogHeader>
          <DialogTitle>
            {t('security.org_entry.sso_title', { organization: waiting.sso.organizationName })}
          </DialogTitle>
          <DialogDescription>{t('security.org_entry.sso_description')}</DialogDescription>
        </DialogHeader>
        <div className="flex min-w-0 flex-col gap-2">
          {waiting.sso.providers.map((provider) => (
            <ChromeButton
              key={provider.id}
              className="w-full min-w-0 justify-center"
              onClick={() => {
                keepSsoTrip(false, provider.id);
                window.location.assign(ssoApi.buildLoginUrl(provider));
              }}
            >
              <SsoPlatformMark platform={provider.platform} kind={provider.kind} />
              <span className="truncate">{provider.name}</span>
            </ChromeButton>
          ))}
        </div>
        <DialogFooter>
          <ChromeButton onClick={() => settle(null)}>{t('common:actions.cancel')}</ChromeButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
