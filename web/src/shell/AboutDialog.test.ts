import { describe, expect, it } from 'vitest';

import type { VersionInfo } from '@/api/version';
import { shouldShowBuildBranch } from '@/shell/AboutDialog';

const productionBuild: VersionInfo = {
  version: '26.0.0.0',
  commit: 'd86fa2d15d68',
  branch: 'main',
  buildEpochSecs: 1_785_087_406,
  buildId: 'gha-12345-1',
  edition: 'enterprise',
};

describe('shouldShowBuildBranch', () => {
  it('shows the branch in a development build', () => {
    expect(shouldShowBuildBranch(productionBuild, true)).toBe(true);
  });

  it('hides the main production branch', () => {
    expect(shouldShowBuildBranch(productionBuild, false)).toBe(false);
  });

  it('keeps a feature branch available in a production-mode preview', () => {
    expect(
      shouldShowBuildBranch(
        { ...productionBuild, version: '26.0.0.0', branch: 'feature/preview' },
        false,
      ),
    ).toBe(true);
  });
});
