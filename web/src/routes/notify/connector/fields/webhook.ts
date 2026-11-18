import { AWS_SETTINGS, TIMEOUT, address, type FieldSpec } from './spec';

/** What a machine receives: the MoleSignal event for it to read, or the template the policy chose. */
const BODY: FieldSpec = { key: 'body', type: 'select', options: ['event', 'template'], defaultValue: 'event', wide: true };

/** The settings of the types that hand notifications to other systems: an incident service, a
 * message topic, or any HTTP endpoint. */
export const WEBHOOK_FIELDS: Record<string, FieldSpec[]> = {
  pagerduty: [{ key: 'routingKey', type: 'password' }, address('apiBaseUrl'), TIMEOUT],
  aws_sns: [...AWS_SETTINGS, { key: 'topicArn', wide: true }, BODY, TIMEOUT],
  webhook: [
    { key: 'url', type: 'password' },
    { key: 'method', type: 'select', options: ['post', 'put', 'patch'], defaultValue: 'post' },
    BODY,
    { key: 'signingSecret', type: 'password', optional: true },
    { key: 'headers', type: 'json', defaultValue: '{}', optional: true },
    TIMEOUT,
  ],
};
