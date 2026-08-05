export function legacyApmUserExperienceTarget(
  pathname: string,
  search = '',
  hash = '',
): string {
  const prefix = '/apm/user-experience';
  const suffix = pathname.startsWith(prefix) ? pathname.slice(prefix.length) : '';
  const destination = rumDestination(suffix);
  return `/rum${destination}${search}${hash}`;
}

export function legacyRumSettingsTarget(
  pathname: string,
  search = '',
  hash = '',
): string {
  const suffix = pathname.startsWith('/rum') ? pathname.slice('/rum'.length) : '';
  return `/rum${rumDestination(suffix)}${search}${hash}`;
}

export function legacyServicesTarget(
  pathname: string,
  search = '',
  hash = '',
): string {
  const suffix = pathname.startsWith('/services')
    ? pathname.slice('/services'.length)
    : '';
  return `/apm/services${suffix}${search}${hash}`;
}

export function apmIndexTarget(search = '', hash = ''): string {
  return `/apm/overview${search}${hash}`;
}

export function legacyVersionCompareTarget(search = '', hash = ''): string {
  return `/apm/deployments${search}${hash}`;
}

function rumDestination(suffix: string): string {
  if (!suffix || suffix === '/') return '/overview';
  if (suffix === '/source-maps') return '/settings/source-maps';
  if (suffix === '/upload-source-maps') return '/settings/source-maps/upload';
  return suffix;
}
