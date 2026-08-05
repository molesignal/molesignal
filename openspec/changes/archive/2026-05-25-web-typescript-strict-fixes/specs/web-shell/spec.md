## ADDED Requirements

### Requirement: Strict TypeScript Build Gate

The `web/` workspace SHALL build cleanly with the project's strict TypeScript configuration enabled: `pnpm -C web typecheck` MUST exit 0 with zero `tsc` errors; `pnpm -C web lint --max-warnings 0` MUST pass; `pnpm -C web build` MUST emit dist. These three commands together form the build gate that CI's `web.yml` workflow enforces.

#### Scenario: `pnpm -C web typecheck` passes with zero errors

- **WHEN** a developer or CI runs `pnpm -C web typecheck`
- **THEN** the command exits with code 0
- **AND** no `TS####` diagnostics are printed to stdout/stderr

#### Scenario: ESLint blocks unused React import regression

- **WHEN** a contributor adds `import * as React from 'react'` to a file that does not reference `React.*`
- **AND** they run `pnpm -C web lint`
- **THEN** lint reports an `@typescript-eslint/no-unused-vars` error and exits non-zero

#### Scenario: exactOptionalPropertyTypes is enforced

- **WHEN** any new code passes `field: SomeType | undefined` to a third-party prop typed as `field?: SomeType`
- **THEN** `pnpm -C web typecheck` MUST fail with TS2375, prompting the author to use the conditional-spread pattern `...(value !== undefined && { field: value })`
