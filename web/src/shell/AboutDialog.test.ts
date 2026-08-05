import { describe, expect, it } from 'vitest';

import type { VersionInfo } from '@/api/version';
import { shouldShowBuildBranch } from '@/shell/AboutDialog';

const stableBuild: VersionInfo = {
  version: '26.0.0.0',
  commit: 'd86fa2d15d68',
  branch: 'main',
  build_epoch_secs: 1_785_087_406,
  build_id: 'gha-12345-1',
  release_channel: 'stable',
  edition: 'enterprise',
};

describe('shouldShowBuildBranch', () => {
  it('shows the branch in a development build', () => {
    expect(shouldShowBuildBranch(stableBuild, true)).toBe(true);
  });

  it('hides a stable production branch', () => {
    expect(shouldShowBuildBranch(stableBuild, false)).toBe(false);
  });

  it('keeps a prerelease branch available in a production-mode preview', () => {
    expect(
      shouldShowBuildBranch(
        { ...stableBuild, version: '26.0.0.0-rc.1', branch: 'release/26' },
        false,
      ),
    ).toBe(true);
  });
});
