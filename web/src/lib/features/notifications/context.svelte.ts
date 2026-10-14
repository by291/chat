import { getContext, setContext } from 'svelte';

import type { PushSubscriptions } from './subscriptions.svelte';

const key = Symbol('background-push');

/** Settings consume the authenticated owner's capabilities; route changes create no new timers. */
class BackgroundPushContext {
  owner = $state<PushSubscriptions | null>(null);
}

export function provideBackgroundPush() {
  const context = new BackgroundPushContext();
  setContext(key, context);
  return context;
}

export function useBackgroundPush() {
  return getContext<BackgroundPushContext>(key);
}
