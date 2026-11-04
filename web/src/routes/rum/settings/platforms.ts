const PLATFORM_NAMES: Record<string, string> = {
  web: 'Web',
  flutter: 'Flutter',
  android: 'Android',
  ios: 'iOS',
};

/** A platform as the debug artifact pages write it: the first letter in capitals, `iOS` as its makers write it. */
export function platformName(platform: string): string {
  return PLATFORM_NAMES[platform] ?? platform.charAt(0).toUpperCase() + platform.slice(1);
}
