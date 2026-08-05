import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  Building2,
  Check,
  Clock3,
  ExternalLink,
  Globe2,
  KeyRound,
  Link2,
  Loader2,
  LockKeyhole,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Users,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as iamApi from '@/api/iam';
import * as resourceSharesApi from '@/api/resourceShares';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { ChromeButton, Pill } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import { Checkbox } from '@/shell/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import { Input } from '@/shell/ui/input';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

type ShareMode = resourceSharesApi.ResourceShareMode;

export interface ResourceShareDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  resourceType: resourceSharesApi.SharedResourceType;
  resourceId: string;
  title: string;
  resourceTags?: readonly string[];
  variableNames?: readonly string[];
  reportFormat?: string;
}

const EXPIRY_OPTIONS = [
  { seconds: 24 * 60 * 60, key: 'one_day' },
  { seconds: 7 * 24 * 60 * 60, key: 'seven_days' },
  { seconds: 30 * 24 * 60 * 60, key: 'thirty_days' },
] as const;

export function ResourceShareDialog({
  open,
  onOpenChange,
  resourceType,
  resourceId,
  title,
  resourceTags = [],
  variableNames = [],
  reportFormat,
}: ResourceShareDialogProps) {
  const { t } = useTranslation('common');
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const canManageGrants = hasPermission('iam.policies.manage', access);
  const canManageSettings = hasPermission('org.settings.manage', access);
  const [mode, setMode] = React.useState<ShareMode>('authenticated');
  const [targetOrganizationId, setTargetOrganizationId] =
    React.useState('');
  const [expiresInSecs, setExpiresInSecs] = React.useState(
    7 * 24 * 60 * 60,
  );
  const [password, setPassword] = React.useState('');
  const [maxViews, setMaxViews] = React.useState('');
  const [allowDownload, setAllowDownload] = React.useState(false);
  const [maxRangeSecs, setMaxRangeSecs] = React.useState(60 * 60);
  const [allowTimeChanges, setAllowTimeChanges] = React.useState(false);
  const [allowVariableChanges, setAllowVariableChanges] =
    React.useState(false);
  const [allowedVariables, setAllowedVariables] = React.useState<string[]>([]);
  const [generatedUrl, setGeneratedUrl] = React.useState('');
  const [copied, setCopied] = React.useState(false);
  const [validationAttempted, setValidationAttempted] =
    React.useState(false);
  const targetOrganizationRef = React.useRef<HTMLSelectElement>(null);
  const passwordInputRef = React.useRef<HTMLInputElement>(null);

  const policyQuery = useQuery({
    queryKey: ['resource-shares', 'policy', access?.organizationId],
    queryFn: resourceSharesApi.getPolicy,
    enabled: open,
    retry: false,
  });
  const shareTargetsQuery = useQuery({
    queryKey: ['iam', 'share-targets', access?.organizationId],
    queryFn: iamApi.listShareTargets,
    enabled: open && canManageGrants,
    retry: false,
  });
  const sharesQuery = useQuery({
    queryKey: ['resource-shares', resourceType, resourceId],
    queryFn: () =>
      resourceSharesApi.list({
        resource_type: resourceType,
        resource_id: resourceId,
      }),
    enabled: open,
  });
  const policy = policyQuery.data;
  const isProductionDashboard =
    resourceType === 'dashboard' &&
    resourceTags.some((tag) =>
      ['prod', 'production'].includes(tag.trim().toLowerCase()),
    );
  const productionPublicDenied =
    Boolean(policy?.deny_production_public_shares) &&
    isProductionDashboard;
  const publicAllowedByResourceType =
    Boolean(policy?.allow_public_links) &&
    (resourceType !== 'dashboard' ||
      Boolean(policy?.allow_public_dashboards));
  const publicAllowed =
    publicAllowedByResourceType && !productionPublicDenied;
  const publicDisabledDescription = productionPublicDenied
    ? t('sharing.modes.production_disabled')
    : !policy?.allow_public_links
      ? t('sharing.modes.public_links_disabled')
      : resourceType === 'dashboard' &&
          !policy?.allow_public_dashboards
        ? t('sharing.modes.public_dashboards_disabled')
        : t('sharing.modes.public_disabled');

  React.useEffect(() => {
    if (!open) return;
    setMode('authenticated');
    setTargetOrganizationId('');
    setExpiresInSecs(7 * 24 * 60 * 60);
    setPassword('');
    setMaxViews('');
    setAllowDownload(false);
    setMaxRangeSecs(60 * 60);
    setAllowTimeChanges(false);
    setAllowVariableChanges(false);
    setAllowedVariables([]);
    setGeneratedUrl('');
    setCopied(false);
    setValidationAttempted(false);
  }, [open, resourceId]);

  React.useEffect(() => {
    if (!open || !policy) return;
    setExpiresInSecs((current) =>
      Math.min(current, policy.max_public_expiry_secs),
    );
  }, [open, policy]);

  React.useEffect(() => {
    if (mode === 'public_link' && !publicAllowed) {
      setMode('authenticated');
    }
  }, [mode, publicAllowed]);

  const createMutation = useMutation({
    mutationFn: () =>
      resourceSharesApi.create({
        resource_type: resourceType,
        resource_id: resourceId,
        share_mode: mode,
        ...(mode === 'public_link' || expiresInSecs > 0
          ? { expires_in_secs: expiresInSecs }
          : {}),
        ...(mode === 'cross_org'
          ? {
              target_organization_id: targetOrganizationId,
              grantee_type: 'organization' as const,
              grantee_id: targetOrganizationId,
            }
          : {}),
        ...(mode === 'public_link' && password.trim()
          ? { password: password.trim() }
          : {}),
        ...(mode === 'public_link' && Number(maxViews) > 0
          ? { max_views: Number(maxViews) }
          : {}),
        ...(mode === 'public_link' && resourceType === 'report'
          ? { allow_download: allowDownload }
          : {}),
        ...(mode === 'public_link' && resourceType === 'dashboard'
          ? {
              constraints: {
                max_time_range_secs: maxRangeSecs,
                allow_time_range_changes: allowTimeChanges,
                allowed_variables: allowedVariables,
                allow_variable_changes: allowVariableChanges,
                auto_refresh_interval_secs: 0,
                watermark: true,
              },
            }
          : {}),
      }),
    onSuccess: async (response) => {
      const absoluteUrl = new URL(
        response.url,
        window.location.origin,
      ).toString();
      setGeneratedUrl(absoluteUrl);
      setCopied(false);
      await queryClient.invalidateQueries({
        queryKey: ['resource-shares', resourceType, resourceId],
      });
      toast.success(t('sharing.created'));
    },
    onError: (error) => {
      const apiError = toApiError(error);
      const knownDescriptions: Record<string, string> = {
        'forbidden: public links are disabled by workspace policy': t(
          'sharing.errors.public_links_disabled',
        ),
        'forbidden: public dashboard links are disabled by workspace policy':
          t('sharing.errors.public_dashboards_disabled'),
        'forbidden: production dashboards cannot be shared publicly': t(
          'sharing.errors.production_dashboard',
        ),
        'forbidden: public CSV download is disabled by workspace policy': t(
          'sharing.errors.csv_download_disabled',
        ),
        'invalid argument: workspace policy requires a password for public reports':
          t('sharing.errors.report_password_required'),
        'invalid argument: public dashboard shares require a password': t(
          'sharing.errors.dashboard_password_required',
        ),
      };
      toast.error(t('sharing.create_failed'), {
        description:
          knownDescriptions[apiError.message] ?? apiError.message,
      });
    },
  });

  const revokeMutation = useMutation({
    mutationFn: resourceSharesApi.revoke,
    onSuccess: async () => {
      setGeneratedUrl('');
      setCopied(false);
      await queryClient.invalidateQueries({
        queryKey: ['resource-shares', resourceType, resourceId],
      });
      toast.success(t('sharing.revoked'));
    },
    onError: (error) =>
      toast.error(t('sharing.revoke_failed'), {
        description: toApiError(error).message,
      }),
  });

  const rotateMutation = useMutation({
    mutationFn: resourceSharesApi.rotate,
    onSuccess: async (response) => {
      setGeneratedUrl(
        new URL(response.url, window.location.origin).toString(),
      );
      setCopied(false);
      await queryClient.invalidateQueries({
        queryKey: ['resource-shares', resourceType, resourceId],
      });
      toast.success(t('sharing.rotated'));
    },
    onError: (error) =>
      toast.error(t('sharing.rotate_failed'), {
        description: toApiError(error).message,
      }),
  });

  const copyGeneratedUrl = async () => {
    if (!generatedUrl) return;
    try {
      await copyText(generatedUrl);
      setCopied(true);
      toast.success(t('sharing.copied'));
    } catch {
      toast.error(t('sharing.copy_failed'));
    }
  };

  const requiresPublicPassword =
    mode === 'public_link' &&
    (resourceType === 'dashboard' ||
      (resourceType === 'report' &&
        Boolean(policy?.require_public_report_password)));
  const targetOrganizationMissing =
    mode === 'cross_org' && !targetOrganizationId;
  const publicPasswordMissing =
    requiresPublicPassword && !password.trim();
  const canSubmit =
    !createMutation.isPending &&
    (mode !== 'public_link' || publicAllowed);
  const createShare = () => {
    setValidationAttempted(true);
    if (targetOrganizationMissing) {
      targetOrganizationRef.current?.focus();
      return;
    }
    if (publicPasswordMissing) {
      passwordInputRef.current?.focus();
      return;
    }
    createMutation.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[min(90vh,820px)] w-[calc(100vw-24px)] max-w-[760px] overflow-y-auto p-0 sm:rounded-xl">
        <DialogHeader className="border-b border-bd-0 px-5 pb-4 pt-5 sm:px-6">
          <div className="flex items-start gap-3 pr-8">
            <div className="grid h-10 w-10 shrink-0 place-items-center rounded-lg border border-indigo/20 bg-indigo-dim text-indigo-soft">
              <Link2 className="h-5 w-5" />
            </div>
            <div className="min-w-0">
              <DialogTitle>{t('sharing.title')}</DialogTitle>
              <DialogDescription className="mt-1 truncate">
                {title}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="space-y-6 px-5 py-5 sm:px-6">
          {generatedUrl ? (
            <GeneratedLink
              url={generatedUrl}
              copied={copied}
              onCopy={() => void copyGeneratedUrl()}
              onReset={() => setGeneratedUrl('')}
            />
          ) : (
            <>
              <section aria-labelledby="share-access-heading">
                <h3
                  id="share-access-heading"
                  className="text-sm font-semibold text-tx-0"
                >
                  {t('sharing.access_scope')}
                </h3>
                <p className="mt-1 text-xs leading-relaxed text-tx-3">
                  {t('sharing.access_scope_hint')}
                </p>
                <div className="mt-3 grid grid-cols-1 gap-2 md:grid-cols-3">
                  <ModeCard
                    checked={mode === 'authenticated'}
                    onClick={() => setMode('authenticated')}
                    icon={<Users className="h-4 w-4" />}
                    title={t('sharing.modes.authenticated')}
                    description={t('sharing.modes.authenticated_hint')}
                  />
                  <ModeCard
                    checked={mode === 'cross_org'}
                    disabled={!canManageGrants}
                    onClick={() => setMode('cross_org')}
                    icon={<Building2 className="h-4 w-4" />}
                    title={t('sharing.modes.cross_org')}
                    description={
                      canManageGrants
                        ? t('sharing.modes.cross_org_hint')
                        : t('sharing.modes.admin_only')
                    }
                  />
                  <ModeCard
                    checked={mode === 'public_link'}
                    disabled={!publicAllowed}
                    onClick={() => setMode('public_link')}
                    icon={<Globe2 className="h-4 w-4" />}
                    title={t('sharing.modes.public_link')}
                    description={
                      publicAllowed
                        ? t('sharing.modes.public_link_hint')
                        : publicDisabledDescription
                    }
                  />
                </div>
                {policy && !publicAllowed && (
                  <div
                    role="alert"
                    className="mt-3 flex items-start gap-3 rounded-lg border border-yellow/30 bg-yellow-dim px-4 py-3 text-yellow-soft"
                  >
                    <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                    <div className="min-w-0">
                      <div className="text-sm font-semibold">
                        {t('sharing.policy_blocked_title')}
                      </div>
                      <p className="mt-1 text-xs leading-relaxed">
                        {publicDisabledDescription}
                      </p>
                      {canManageSettings ? (
                        <Link
                          to="/settings/general"
                          className="mt-2 inline-flex items-center gap-1 text-xs font-semibold text-current underline underline-offset-2"
                        >
                          {t('sharing.policy_blocked_action')}
                          <ExternalLink className="h-3 w-3" />
                        </Link>
                      ) : (
                        <p className="mt-2 text-xs">
                          {t('sharing.policy_blocked_admin')}
                        </p>
                      )}
                    </div>
                  </div>
                )}
              </section>

              {mode === 'cross_org' && (
                <Field
                  label={t('sharing.target_organization')}
                  hint={t('sharing.target_organization_hint')}
                  >
                    <select
                      ref={targetOrganizationRef}
                      value={targetOrganizationId}
                      onChange={(event) =>
                        setTargetOrganizationId(event.target.value)
                      }
                      aria-invalid={
                        validationAttempted && targetOrganizationMissing
                      }
                      aria-describedby={
                        validationAttempted && targetOrganizationMissing
                          ? 'resource-share-organization-error'
                          : undefined
                      }
                      className={cn(
                        'h-11 w-full rounded-md border border-bd-1 bg-bg-1 px-3 text-sm text-tx-1 outline-none focus:border-indigo',
                        validationAttempted &&
                          targetOrganizationMissing &&
                          'border-red focus:border-red',
                      )}
                    >
                    <option value="">
                      {t('sharing.select_organization')}
                    </option>
                    {(shareTargetsQuery.data ?? []).map((organization) => (
                      <option key={organization.id} value={organization.id}>
                        {organization.name}
                      </option>
                    ))}
                  </select>
                  {validationAttempted && targetOrganizationMissing && (
                    <p
                      id="resource-share-organization-error"
                      className="mt-1.5 text-xs text-red"
                    >
                      {t('sharing.validation.organization_required')}
                    </p>
                  )}
                </Field>
              )}

              {mode === 'public_link' && (
                <div className="space-y-5">
                  <div className="flex gap-3 rounded-lg border border-yellow/30 bg-yellow-dim px-4 py-3 text-yellow-soft">
                    <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                    <div>
                      <div className="text-sm font-semibold">
                        {t('sharing.public_warning_title')}
                      </div>
                      <p className="mt-1 text-xs leading-relaxed">
                        {t('sharing.public_warning')}
                      </p>
                    </div>
                  </div>

                  {resourceType === 'report' && (
                    <div className="flex items-center gap-3 rounded-lg border border-bd-0 bg-bg-2 px-4 py-3">
                      <ShieldCheck className="h-4 w-4 shrink-0 text-green-soft" />
                      <p className="text-xs leading-relaxed text-tx-2">
                        {t('sharing.report_snapshot_hint')}
                      </p>
                    </div>
                  )}

                  <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
                    <Field
                      label={t('sharing.expiry')}
                      hint={t('sharing.expiry_hint')}
                    >
                      <select
                        value={expiresInSecs}
                        onChange={(event) =>
                          setExpiresInSecs(Number(event.target.value))
                        }
                        className="h-11 w-full rounded-md border border-bd-1 bg-bg-1 px-3 text-sm text-tx-1 outline-none focus:border-indigo"
                      >
                        {EXPIRY_OPTIONS.filter(
                          (option) =>
                            option.seconds <=
                            (policy?.max_public_expiry_secs ??
                              7 * 24 * 60 * 60),
                        ).map((option) => (
                          <option key={option.seconds} value={option.seconds}>
                            {t(`sharing.expiry_options.${option.key}`)}
                          </option>
                        ))}
                      </select>
                    </Field>
                    <Field
                      label={t('sharing.max_views')}
                      hint={t('sharing.max_views_hint')}
                    >
                      <Input
                        type="number"
                        inputMode="numeric"
                        min={1}
                        value={maxViews}
                        placeholder={t('sharing.unlimited')}
                        onChange={(event) => setMaxViews(event.target.value)}
                        className="h-11"
                      />
                    </Field>
                  </div>

                  <Field
                    label={t('sharing.password')}
                    hint={
                      requiresPublicPassword
                        ? t(
                            resourceType === 'dashboard'
                              ? 'sharing.dashboard_password_required'
                              : 'sharing.password_required',
                          )
                        : t('sharing.password_hint')
                    }
                  >
                    <div className="relative">
                      <KeyRound className="pointer-events-none absolute left-3 top-3.5 h-4 w-4 text-tx-3" />
                      <Input
                        ref={passwordInputRef}
                        type="password"
                        autoComplete="new-password"
                        value={password}
                        onChange={(event) => setPassword(event.target.value)}
                        aria-invalid={
                          validationAttempted && publicPasswordMissing
                        }
                        aria-describedby={
                          validationAttempted && publicPasswordMissing
                            ? 'resource-share-password-error'
                            : undefined
                        }
                        className={cn(
                          'h-11 pl-10',
                          validationAttempted &&
                            publicPasswordMissing &&
                            'border-red focus-visible:border-red',
                        )}
                        required={requiresPublicPassword}
                      />
                    </div>
                    {validationAttempted && publicPasswordMissing && (
                      <p
                        id="resource-share-password-error"
                        className="mt-1.5 text-xs text-red"
                      >
                        {t(
                          resourceType === 'dashboard'
                            ? 'sharing.validation.dashboard_password_required'
                            : 'sharing.validation.password_required',
                        )}
                      </p>
                    )}
                  </Field>

                  {resourceType === 'dashboard' ? (
                    <DashboardConstraints
                      variableNames={variableNames}
                      maxRangeSecs={maxRangeSecs}
                      onMaxRangeChange={setMaxRangeSecs}
                      allowTimeChanges={allowTimeChanges}
                      onAllowTimeChanges={setAllowTimeChanges}
                      allowVariableChanges={allowVariableChanges}
                      onAllowVariableChanges={setAllowVariableChanges}
                      allowedVariables={allowedVariables}
                      onAllowedVariables={setAllowedVariables}
                    />
                  ) : (
                    <ToggleRow
                      label={t('sharing.allow_download')}
                      description={
                        reportFormat === 'csv' &&
                        !policy?.allow_public_csv_download
                          ? t('sharing.csv_download_disabled')
                          : t('sharing.allow_download_hint')
                      }
                      checked={allowDownload}
                      disabled={
                        reportFormat === 'csv' &&
                        !policy?.allow_public_csv_download
                      }
                      onCheckedChange={setAllowDownload}
                    />
                  )}
                </div>
              )}

              {mode !== 'public_link' && (
                <Field
                  label={t('sharing.expiry')}
                  hint={t('sharing.authenticated_expiry_hint')}
                >
                  <select
                    value={expiresInSecs}
                    onChange={(event) =>
                      setExpiresInSecs(Number(event.target.value))
                    }
                    className="h-11 w-full rounded-md border border-bd-1 bg-bg-1 px-3 text-sm text-tx-1 outline-none focus:border-indigo md:max-w-xs"
                  >
                    {EXPIRY_OPTIONS.map((option) => (
                      <option key={option.seconds} value={option.seconds}>
                        {t(`sharing.expiry_options.${option.key}`)}
                      </option>
                    ))}
                  </select>
                </Field>
              )}
            </>
          )}

          <ExistingShares
            shares={sharesQuery.data ?? []}
            loading={sharesQuery.isLoading}
            revokingId={
              revokeMutation.isPending
                ? (revokeMutation.variables ?? null)
                : null
            }
            rotatingId={
              rotateMutation.isPending
                ? (rotateMutation.variables ?? null)
                : null
            }
            onRevoke={(id) => revokeMutation.mutate(id)}
            onRotate={(id) => rotateMutation.mutate(id)}
          />
        </div>

        <DialogFooter className="sticky bottom-0 border-t border-bd-0 bg-bg-1 px-5 py-4 sm:px-6">
          <ChromeButton onClick={() => onOpenChange(false)}>
            {t('actions.close')}
          </ChromeButton>
          {!generatedUrl && (
            <ChromeButton
              variant="primary"
              disabled={!canSubmit}
              onClick={createShare}
            >
              {createMutation.isPending ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Link2 className="h-4 w-4" />
              )}
              {t('sharing.create_link')}
            </ChromeButton>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ModeCard({
  checked,
  disabled,
  onClick,
  icon,
  title,
  description,
}: {
  checked: boolean;
  disabled?: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  title: string;
  description: string;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'min-h-[116px] rounded-lg border p-3.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
        checked
          ? 'border-indigo bg-indigo-dim'
          : 'border-bd-0 bg-bg-1 hover:border-bd-2 hover:bg-bg-2',
        disabled && 'cursor-not-allowed opacity-50',
      )}
    >
      <span
        className={cn(
          'grid h-8 w-8 place-items-center rounded-md',
          checked ? 'bg-indigo text-white' : 'border border-bd-0 bg-bg-2 text-tx-2',
        )}
      >
        {icon}
      </span>
      <span className="mt-3 block text-sm font-semibold text-tx-0">
        {title}
      </span>
      <span className="mt-1 block text-xs leading-relaxed text-tx-3">
        {description}
      </span>
    </button>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="block text-sm font-semibold text-tx-1">{label}</span>
      {hint && (
        <span className="mt-1 block text-xs leading-relaxed text-tx-3">
          {hint}
        </span>
      )}
      <span className="mt-2 block">{children}</span>
    </label>
  );
}

function ToggleRow({
  label,
  description,
  checked,
  disabled,
  onCheckedChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  disabled?: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <div className="flex min-h-14 items-center gap-4 rounded-lg border border-bd-0 bg-bg-1 px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="text-sm font-semibold text-tx-1">{label}</div>
        <div className="mt-0.5 text-xs leading-relaxed text-tx-3">
          {description}
        </div>
      </div>
      <Switch
        checked={checked}
        disabled={disabled}
        onCheckedChange={onCheckedChange}
        aria-label={label}
      />
    </div>
  );
}

function DashboardConstraints({
  variableNames,
  maxRangeSecs,
  onMaxRangeChange,
  allowTimeChanges,
  onAllowTimeChanges,
  allowVariableChanges,
  onAllowVariableChanges,
  allowedVariables,
  onAllowedVariables,
}: {
  variableNames: readonly string[];
  maxRangeSecs: number;
  onMaxRangeChange: (value: number) => void;
  allowTimeChanges: boolean;
  onAllowTimeChanges: (value: boolean) => void;
  allowVariableChanges: boolean;
  onAllowVariableChanges: (value: boolean) => void;
  allowedVariables: string[];
  onAllowedVariables: (value: string[]) => void;
}) {
  const { t } = useTranslation('common');
  return (
    <section className="space-y-3 border-t border-bd-0 pt-5">
      <div>
        <h3 className="text-sm font-semibold text-tx-0">
          {t('sharing.dashboard_restrictions')}
        </h3>
        <p className="mt-1 text-xs leading-relaxed text-tx-3">
          {t('sharing.dashboard_restrictions_hint')}
        </p>
      </div>
      <Field label={t('sharing.max_time_range')}>
        <select
          value={maxRangeSecs}
          onChange={(event) => onMaxRangeChange(Number(event.target.value))}
          className="h-11 w-full rounded-md border border-bd-1 bg-bg-1 px-3 text-sm text-tx-1 outline-none focus:border-indigo md:max-w-xs"
        >
          <option value={3600}>{t('sharing.time_ranges.one_hour')}</option>
          <option value={6 * 3600}>{t('sharing.time_ranges.six_hours')}</option>
          <option value={24 * 3600}>
            {t('sharing.time_ranges.twenty_four_hours')}
          </option>
        </select>
      </Field>
      <ToggleRow
        label={t('sharing.allow_time_changes')}
        description={t('sharing.allow_time_changes_hint')}
        checked={allowTimeChanges}
        onCheckedChange={onAllowTimeChanges}
      />
      {variableNames.length > 0 && (
        <div className="rounded-lg border border-bd-0 bg-bg-1 p-4">
          <div className="text-sm font-semibold text-tx-1">
            {t('sharing.allowed_variables')}
          </div>
          <div className="mt-1 text-xs leading-relaxed text-tx-3">
            {t('sharing.allowed_variables_hint')}
          </div>
          <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
            {variableNames.map((name) => {
              const checked = allowedVariables.includes(name);
              return (
                <label
                  key={name}
                  className="flex min-h-11 items-center gap-2 rounded-md border border-bd-0 px-3 text-sm text-tx-1"
                >
                  <Checkbox
                    checked={checked}
                    onCheckedChange={(next) =>
                      onAllowedVariables(
                        next
                          ? [...allowedVariables, name]
                          : allowedVariables.filter((value) => value !== name),
                      )
                    }
                  />
                  <span className="truncate font-mono text-xs">{name}</span>
                </label>
              );
            })}
          </div>
          <div className="mt-3">
            <ToggleRow
              label={t('sharing.allow_variable_changes')}
              description={t('sharing.allow_variable_changes_hint')}
              checked={allowVariableChanges}
              disabled={allowedVariables.length === 0}
              onCheckedChange={onAllowVariableChanges}
            />
          </div>
        </div>
      )}
    </section>
  );
}

function GeneratedLink({
  url,
  copied,
  onCopy,
  onReset,
}: {
  url: string;
  copied: boolean;
  onCopy: () => void;
  onReset: () => void;
}) {
  const { t } = useTranslation('common');
  return (
    <section className="rounded-xl border border-green/30 bg-green-dim p-4">
      <div className="flex items-center gap-2 text-green-soft">
        <Check className="h-4 w-4" />
        <h3 className="text-sm font-semibold">{t('sharing.link_ready')}</h3>
      </div>
      <p className="mt-1 text-xs leading-relaxed text-tx-2">
        {t('sharing.link_ready_hint')}
      </p>
      <div className="mt-3 flex flex-col gap-2 sm:flex-row">
        <div className="min-w-0 flex-1 truncate rounded-md border border-bd-1 bg-bg-1 px-3 py-2.5 font-mono text-xs text-tx-1">
          {url}
        </div>
        <CopyIconButton
          onClick={onCopy}
          label={t('sharing.copy_link')}
          copied={copied}
          copiedLabel={t('sharing.copied')}
          className="h-11 w-11 bg-indigo text-white enabled:hover:bg-indigo-soft sm:h-8 sm:w-8"
          iconClassName="h-4 w-4"
          wrapperClassName="self-end sm:self-auto"
        />
      </div>
      <button
        type="button"
        onClick={onReset}
        className="mt-3 text-xs font-semibold text-green-soft hover:text-tx-0"
      >
        {t('sharing.create_another')}
      </button>
    </section>
  );
}

function ExistingShares({
  shares,
  loading,
  revokingId,
  rotatingId,
  onRevoke,
  onRotate,
}: {
  shares: resourceSharesApi.ResourceShare[];
  loading: boolean;
  revokingId: string | null;
  rotatingId: string | null;
  onRevoke: (id: string) => void;
  onRotate: (id: string) => void;
}) {
  const { t, i18n } = useTranslation('common');
  const copyExistingUrl = async (url: string) => {
    try {
      await copyText(url);
      toast.success(t('sharing.copied'));
    } catch {
      toast.error(t('sharing.copy_failed'));
    }
  };
  if (loading) {
    return (
      <div className="flex items-center gap-2 border-t border-bd-0 pt-5 text-xs text-tx-3">
        <Loader2 className="h-4 w-4 animate-spin" />
        {t('status.loading')}
      </div>
    );
  }
  if (shares.length === 0) return null;
  return (
    <section className="border-t border-bd-0 pt-5">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-tx-0">
            {t('sharing.existing')}
          </h3>
          <p className="mt-1 text-xs text-tx-3">
            {t('sharing.existing_hint')}
          </p>
        </div>
        <Pill tone="dim">{shares.length}</Pill>
      </div>
      <div className="mt-3 divide-y divide-bd-0 overflow-hidden rounded-lg border border-bd-0">
        {shares.map((share) => {
          const active =
            share.enabled &&
            !share.revoked_at &&
            (!share.expires_at || share.expires_at > Date.now() * 1000);
          const shareUrl = active && share.url
            ? new URL(share.url, window.location.origin).toString()
            : null;
          return (
            <div
              key={share.id}
              className="flex flex-col gap-3 bg-bg-1 px-4 py-3 sm:flex-row sm:items-center"
            >
              <div className="flex min-w-0 flex-1 items-start gap-3">
                <span className="mt-0.5 grid h-8 w-8 shrink-0 place-items-center rounded-md border border-bd-0 bg-bg-2 text-tx-2">
                  {share.share_mode === 'public_link' ? (
                    <Globe2 className="h-4 w-4" />
                  ) : share.share_mode === 'cross_org' ? (
                    <Building2 className="h-4 w-4" />
                  ) : (
                    <LockKeyhole className="h-4 w-4" />
                  )}
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-semibold text-tx-1">
                      {t(`sharing.modes.${share.share_mode}`)}
                    </span>
                    <Pill tone={active ? 'green' : 'dim'}>
                      {active
                        ? t('status.enabled')
                        : t('status.disabled')}
                    </Pill>
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-tx-3">
                    <span className="inline-flex items-center gap-1">
                      <Clock3 className="h-3 w-3" />
                      {share.expires_at
                        ? new Intl.DateTimeFormat(i18n.language, {
                            dateStyle: 'medium',
                            timeStyle: 'short',
                          }).format(new Date(share.expires_at / 1000))
                        : t('sharing.no_expiry')}
                    </span>
                    <span>
                      {t('sharing.views', { count: share.view_count })}
                    </span>
                  </div>
                  {(shareUrl || active) && (
                    <div className="mt-2 flex min-w-0 flex-col gap-2 sm:flex-row sm:items-center">
                      {shareUrl ? (
                        <div
                          className="min-w-0 flex-1 truncate rounded-md border border-bd-0 bg-bg-2 px-2.5 py-1.5 font-mono text-xs text-tx-2"
                          title={shareUrl}
                        >
                          {shareUrl}
                        </div>
                      ) : (
                        <p className="min-w-0 flex-1 text-xs text-yellow-soft">
                          {t('sharing.rotate_to_show_link')}
                        </p>
                      )}
                      <div className="flex flex-wrap items-center gap-2 sm:shrink-0 sm:flex-nowrap sm:justify-end">
                        {shareUrl && (
                          <CopyIconButton
                            label={t('sharing.copy_existing_link')}
                            onClick={() => void copyExistingUrl(shareUrl)}
                          />
                        )}
                        {active && (
                          <>
                            <ChromeButton
                              size="sm"
                              disabled={rotatingId === share.id}
                              onClick={() => onRotate(share.id)}
                            >
                              {rotatingId === share.id ? (
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                              ) : (
                                <RefreshCw className="h-3.5 w-3.5" />
                              )}
                              {t('actions.rotate')}
                            </ChromeButton>
                            <ChromeButton
                              size="sm"
                              disabled={revokingId === share.id}
                              onClick={() => onRevoke(share.id)}
                            >
                              {revokingId === share.id ? (
                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                              ) : (
                                <Trash2 className="h-3.5 w-3.5" />
                              )}
                              {t('sharing.stop')}
                            </ChromeButton>
                          </>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              </div>
            </div>
          );
        })}
      </div>
      {!publicAllowedFromShares(shares) && (
        <Link
          to="/settings/general"
          className="mt-3 inline-flex items-center gap-1 text-xs font-semibold text-indigo-soft hover:text-tx-0"
        >
          {t('sharing.workspace_policy')}
          <ExternalLink className="h-3 w-3" />
        </Link>
      )}
    </section>
  );
}

function publicAllowedFromShares(
  shares: resourceSharesApi.ResourceShare[],
): boolean {
  return shares.some((share) => share.share_mode === 'public_link');
}

async function copyText(value: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return;
    } catch {
      // Fall through when browser permission is unavailable.
    }
  }
  const input = document.createElement('textarea');
  input.value = value;
  input.readOnly = true;
  input.style.position = 'fixed';
  input.style.left = '-9999px';
  document.body.appendChild(input);
  input.select();
  const copied = document.execCommand('copy');
  input.remove();
  if (!copied) throw new Error('copy failed');
}
