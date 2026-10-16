/**
 * What a link into the page asks of the subscription, as its fragment says:
 * links in a confirmation, manage or unsubscribe message carry a token, and
 * the return from Add to Slack says only how it went.
 */
export type SubscriptionAction =
  | { kind: 'confirm' | 'unsubscribe' | 'manage'; token: string }
  | { kind: 'slack_connected' | 'slack_failed' }
  | null;

export function subscriptionAction(hash: string): SubscriptionAction {
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  const action = params.get('subscription_action');
  const token = params.get('subscription_token');

  if ((action === 'confirm' || action === 'unsubscribe' || action === 'manage') && token) {
    return { kind: action, token };
  }
  if (action === 'slack_connected' || action === 'slack_failed') {
    return { kind: action };
  }
  return null;
}

/** The components a subscription names: none is the whole page. */
export function chosenComponentIds(scope: 'all' | 'selected', chosen: string[]): string[] {
  return scope === 'all' ? [] : chosen;
}
