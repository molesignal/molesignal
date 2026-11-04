import { describe, expect, it } from 'vitest';

import type { VersionInfo } from '@/api/version';
import { shouldShowBuildBranch } from '@/shell/AboutDialog';

const stableBuild: VersionInfo = {
  version: '26.0.0.0',
  commit: 'd86fa2d15d68',
  branch: 'main',
  buildEpochSecs: 1_785_087_406,
  buildId: 'gha-12345-1',
  releaseChannel: 'stable',
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
