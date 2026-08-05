import type { ErrorObject, ValidateFunction } from 'ajv';
import Ajv2020 from 'ajv/dist/2020.js';

import type { DashboardDefinition } from '../schema';
import {
  dashboardAuthoringV1Schema,
  dashboardModelV2Schema,
} from './generated/schemas';
import type { DashboardAuthoringV1Contract } from './types';

const MAX_ISSUES = 20;
const ajv = new Ajv2020({
  allErrors: true,
  allowUnionTypes: true,
  strict: true,
});

const validateModel = ajv.compile(dashboardModelV2Schema);
const validateAuthoring = ajv.compile(dashboardAuthoringV1Schema);

export interface DashboardContractIssue {
  code: string;
  path: string;
  message: string;
  retryable: boolean;
}

export type DashboardContractValidation<T> =
  | { valid: true; value: T; issues: [] }
  | { valid: false; issues: DashboardContractIssue[] };

export function validateDashboardModelContract(
  value: unknown,
): DashboardContractValidation<DashboardDefinition> {
  return validateContract<DashboardDefinition>(validateModel, value);
}

export function validateDashboardAuthoringContract(
  value: unknown,
): DashboardContractValidation<DashboardAuthoringV1Contract> {
  return validateContract<DashboardAuthoringV1Contract>(
    validateAuthoring,
    value,
  );
}

function validateContract<T>(
  validate: ValidateFunction,
  value: unknown,
): DashboardContractValidation<T> {
  if (validate(value)) return { valid: true, value: value as T, issues: [] };
  return {
    valid: false,
    issues: (validate.errors ?? []).slice(0, MAX_ISSUES).map(toIssue),
  };
}

function toIssue(error: ErrorObject): DashboardContractIssue {
  const unsupportedAuthoringVersion =
    error.keyword === 'const' && error.instancePath === '/authoringVersion';
  return {
    code: unsupportedAuthoringVersion
      ? 'UNSUPPORTED_AUTHORING_VERSION'
      : issueCode(error.keyword),
    path: error.instancePath,
    message: `${error.instancePath || '/'} ${error.message ?? 'is invalid'}`,
    retryable: true,
  };
}

function issueCode(keyword: string): string {
  switch (keyword) {
    case 'additionalItems':
    case 'additionalProperties':
    case 'unevaluatedItems':
    case 'unevaluatedProperties':
      return 'CONTRACT_ADDITIONAL_PROPERTY';
    case 'const':
      return 'CONTRACT_CONST';
    case 'enum':
      return 'CONTRACT_ENUM';
    case 'required':
      return 'CONTRACT_REQUIRED';
    case 'type':
      return 'CONTRACT_TYPE';
    case 'maxItems':
    case 'maxLength':
    case 'maxProperties':
    case 'maximum':
    case 'exclusiveMaximum':
      return 'CONTRACT_MAXIMUM';
    case 'minItems':
    case 'minLength':
    case 'minProperties':
    case 'minimum':
    case 'exclusiveMinimum':
      return 'CONTRACT_MINIMUM';
    case 'oneOf':
      return 'CONTRACT_ONE_OF';
    case 'anyOf':
      return 'CONTRACT_ANY_OF';
    case 'pattern':
      return 'CONTRACT_PATTERN';
    case 'uniqueItems':
      return 'CONTRACT_UNIQUE_ITEMS';
    case '$ref':
      return 'CONTRACT_REFERENCE';
    default:
      return 'CONTRACT_VALIDATION_FAILED';
  }
}
