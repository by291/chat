import { expect, test } from 'vitest';

import { PwaLifecycle } from '../../src/lib/features/pwa/lifecycle.svelte';

function browserFixture() {
  const activations: unknown[] = [];
  const registration = Object.assign(new EventTarget(), {
    waiting: {
      postMessage: (message: unknown) => {
        activations.push(message);
      },
    },
    installing: null,
  });
  const container = Object.assign(new EventTarget(), {
    getRegistration: async () => registration,
    ready: Promise.resolve(registration),
  });
  const events = new EventTarget();
  let reloads = 0;
  const lifecycle = new PwaLifecycle({
    events,
    serviceWorker: container,
    standalone: () => false,
    reload: () => {
      reloads++;
    },
  });

  return { lifecycle, events, container, activations, reloads: () => reloads };
}

test('a waiting update leaves drafts uploads and submissions intact until explicit confirmation', async () => {
  const fixture = browserFixture();
  await fixture.lifecycle.start();

  await fixture.lifecycle.requestUpdate({
    hasDraft: true,
    hasUpload: true,
    hasSubmission: true,
  });

  expect(fixture.lifecycle.updateAvailable).toBe(true);
  expect(fixture.lifecycle.confirmationRequired).toBe(true);
  expect(fixture.activations).toEqual([]);
  expect(fixture.reloads()).toBe(0);
  fixture.lifecycle.cancelUpdate();
  expect(fixture.lifecycle.confirmationRequired).toBe(false);
  expect(fixture.activations).toEqual([]);
  fixture.lifecycle.stop();
});

test('the confirmed user action activates once and reloads only after controller handoff', async () => {
  const fixture = browserFixture();
  await fixture.lifecycle.start();
  await fixture.lifecycle.requestUpdate({
    hasDraft: true,
    hasUpload: false,
    hasSubmission: false,
  });

  fixture.lifecycle.confirmUpdate();

  expect(fixture.activations).toEqual([{ type: 'activate-update' }]);
  expect(fixture.reloads()).toBe(0);
  fixture.container.dispatchEvent(new Event('controllerchange'));
  expect(fixture.reloads()).toBe(1);
  fixture.container.dispatchEvent(new Event('controllerchange'));
  expect(fixture.reloads()).toBe(1);
  fixture.lifecycle.stop();
});

test('unrequested controller changes never reload the current authenticated lifetime', async () => {
  const fixture = browserFixture();
  await fixture.lifecycle.start();

  fixture.container.dispatchEvent(new Event('controllerchange'));

  expect(fixture.reloads()).toBe(0);
  expect(fixture.activations).toEqual([]);
  fixture.lifecycle.stop();
});

test('the deferred install prompt runs only from a user action and teardown releases its listeners', async () => {
  const fixture = browserFixture();
  await fixture.lifecycle.start();
  let prompts = 0;
  const event = Object.assign(
    new Event('beforeinstallprompt', { cancelable: true }),
    {
      prompt: async () => {
        prompts++;
      },
      userChoice: Promise.resolve({ outcome: 'dismissed' }),
    },
  );
  fixture.events.dispatchEvent(event);
  expect(event.defaultPrevented).toBe(true);
  expect(prompts).toBe(0);

  await fixture.lifecycle.install();

  expect(prompts).toBe(1);
  fixture.lifecycle.stop();
  const after = new Event('beforeinstallprompt', { cancelable: true });
  fixture.events.dispatchEvent(after);
  expect(after.defaultPrevented).toBe(false);
});

test('late registration after stop cannot restore update state or activate a worker', async () => {
  const fixture = browserFixture();
  let finish!: (
    value: Awaited<ReturnType<typeof fixture.container.getRegistration>>,
  ) => void;
  const previous = await fixture.container.getRegistration();
  fixture.container.getRegistration = () =>
    new Promise((resolve) => {
      finish = resolve;
    });
  const start = fixture.lifecycle.start();

  fixture.lifecycle.stop();
  finish(previous);
  await start;

  expect(fixture.lifecycle.updateAvailable).toBe(false);
  await fixture.lifecycle.requestUpdate({
    hasDraft: false,
    hasUpload: false,
    hasSubmission: false,
  });
  expect(fixture.activations).toEqual([]);
});
