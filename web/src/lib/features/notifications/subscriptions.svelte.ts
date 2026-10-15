import type { NotificationApi, PushActivity, PushBinding } from './api';

type BrowserSubscription = Pick<PushSubscription, 'toJSON' | 'unsubscribe'>;
type Registration = {
  pushManager: {
    getSubscription(): Promise<BrowserSubscription | null>;
    subscribe(
      options: PushSubscriptionOptionsInit,
    ): Promise<BrowserSubscription>;
  };
  getNotifications(): Promise<ReadonlyArray<Pick<Notification, 'close'>>>;
};

type PushPorts = {
  events: EventTarget;
  documentEvents: EventTarget;
  permission(): NotificationPermission;
  requestPermission(): Promise<NotificationPermission>;
  registration(
    signal?: AbortSignal,
    waitForActive?: boolean,
  ): Promise<Registration | undefined>;
  activity(): Pick<PushActivity, 'visible' | 'focused'>;
};

const WORKER_READY_TIMEOUT_MS = 15_000;
const ACTIVITY_REFRESH_INTERVAL_MS = 10_000;

/** Registration begins at window.load; cancellation must not retain a disposed owner. */
async function activeRegistration(
  signal: AbortSignal,
): Promise<ServiceWorkerRegistration> {
  let abort!: () => void;
  let deadline!: ReturnType<typeof setTimeout>;
  const cancelled = new Promise<never>((_, reject) => {
    abort = () => reject(new DOMException('Push lifetime ended', 'AbortError'));
    signal.addEventListener('abort', abort, { once: true });
    deadline = setTimeout(
      () => reject(new Error('服务工作线程尚未就绪，请刷新页面后重试。')),
      WORKER_READY_TIMEOUT_MS,
    );
    if (signal.aborted) abort();
  });

  try {
    return await Promise.race([navigator.serviceWorker.ready, cancelled]);
  } finally {
    clearTimeout(deadline);
    signal.removeEventListener('abort', abort);
  }
}

function browserPorts(): PushPorts | undefined {
  if (
    typeof window === 'undefined' ||
    !window.isSecureContext ||
    !('serviceWorker' in navigator) ||
    typeof PushManager === 'undefined' ||
    typeof Notification === 'undefined'
  ) {
    return undefined;
  }
  return {
    events: window,
    documentEvents: document,
    permission: () => Notification.permission,
    requestPermission: () => Notification.requestPermission(),
    registration: async (signal, waitForActive = false) => {
      const registration = await navigator.serviceWorker.getRegistration();
      if (
        waitForActive &&
        signal &&
        registration?.active?.state !== 'activated'
      ) {
        return activeRegistration(signal);
      }
      return registration;
    },
    activity: () => ({
      visible: document.visibilityState === 'visible',
      focused: document.hasFocus(),
    }),
  };
}

// Browser subscription/unsubscribe operations outlive an authenticated Svelte tree.
// Serialize their handoff so disposed owners cannot unsubscribe a successor's capability.
// This SPA disables SSR; move this browser-only coordination before enabling SSR.
let capabilityTail: Promise<void> = Promise.resolve();

function serialized<T>(operation: () => Promise<T>): Promise<T> {
  const result = capabilityTail.then(operation, operation);
  // Preserve queue continuity while returning the original failure to its caller.
  capabilityTail = result.then(
    () => {},
    () => {},
  );
  return result;
}

/** Owns one authenticated push lifetime; binding acknowledgements are private capability handles. */
export class PushSubscriptions {
  configured = $state(false);
  supported = $state(false);
  permission = $state<NotificationPermission | 'unsupported'>('unsupported');
  enabled = $state(false);
  pending = $state(false);
  error = $state('');

  private active = false;
  private revokeCapability = false;
  private generation = 0;
  private controller = new AbortController();
  private registration?: Registration;
  private subscription?: BrowserSubscription;
  private binding?: PushBinding;
  private publicKey?: string;
  private cleanups: Array<() => void> = [];
  private timer?: ReturnType<typeof setInterval>;
  private activity: PushActivity = {
    visible: false,
    focused: false,
    conversation_id: null,
  };
  private activityTail: Promise<void> = Promise.resolve();

  constructor(
    private api: NotificationApi,
    private ports?: PushPorts,
  ) {}

  /** Rebind a pre-existing granted capability; never request permission or silently create one. */
  async start(): Promise<void> {
    if (this.active) return;
    this.active = true;
    this.revokeCapability = false;
    const generation = ++this.generation;
    this.controller = new AbortController();
    this.pending = true;
    this.ports ??= browserPorts();

    try {
      const config = await this.api.config(this.controller.signal);
      if (!this.current(generation)) return;
      this.configured = config.enabled;
      this.publicKey = config.vapid_public_key ?? undefined;
      if (!config.enabled || !this.ports) return;
      this.permission = this.ports.permission();
      this.registration = await this.ports.registration(
        this.controller.signal,
        true,
      );
      if (!this.current(generation) || !this.registration) return;
      this.supported = true;
      const refresh = () => {
        if (this.ports) {
          this.setActivity({ ...this.activity, ...this.ports.activity() });
        }
      };
      this.listen(this.ports.events, 'focus', refresh);
      this.listen(this.ports.events, 'blur', refresh);
      this.listen(this.ports.documentEvents, 'visibilitychange', refresh);
      this.listen(this.ports.events, 'pagehide', () =>
        this.setActivity({ ...this.activity, visible: false, focused: false }),
      );
      this.timer = setInterval(refresh, ACTIVITY_REFRESH_INTERVAL_MS);
      await serialized(async () => {
        if (!this.current(generation)) return;
        const subscription =
          await this.registration!.pushManager.getSubscription();
        if (
          subscription &&
          this.permission === 'granted' &&
          this.current(generation)
        ) {
          await this.bind(subscription, generation);
        }
      });
    } catch (error) {
      if (this.current(generation)) this.error = this.failure(error);
    } finally {
      if (this.current(generation)) this.pending = false;
    }
  }

  /** Call directly from a button: the permission request precedes every await/queued operation. */
  async enable(): Promise<void> {
    if (
      !this.active ||
      this.pending ||
      !this.supported ||
      !this.configured ||
      !this.ports
    ) {
      return;
    }
    const generation = this.generation;
    const permission =
      this.ports.permission() === 'default'
        ? this.ports.requestPermission()
        : Promise.resolve(this.ports.permission());
    this.pending = true;
    this.error = '';

    try {
      const granted = await permission;
      if (!this.current(generation)) return;
      this.permission = granted;
      if (granted !== 'granted') return;
      await serialized(async () => {
        if (!this.current(generation)) return;
        const registration = this.registration!;
        const subscription =
          (await registration.pushManager.getSubscription()) ??
          (await registration.pushManager.subscribe({
            userVisibleOnly: true,
            applicationServerKey: this.key(),
          }));
        if (!this.current(generation)) {
          if (this.revokeCapability) await subscription.unsubscribe();
          return;
        }
        await this.bind(subscription, generation);
      });
    } catch (error) {
      if (this.current(generation)) this.error = this.failure(error);
    } finally {
      if (this.current(generation)) this.pending = false;
    }
  }

  /** Remove server ownership and browser capability even if provider/browser cleanup fails. */
  async disable(): Promise<void> {
    if (!this.active || this.pending) return;
    const generation = this.generation;
    this.pending = true;
    this.error = '';
    const binding = this.binding;
    const subscription = this.subscription;
    this.enabled = false;
    this.binding = undefined;
    this.subscription = undefined;

    try {
      await serialized(async () => {
        try {
          if (binding && this.current(generation)) {
            await this.api.remove(binding.id, this.controller.signal);
          }
        } finally {
          await this.clearBrowser(subscription, this.registration);
        }
      });
    } catch (error) {
      if (this.current(generation)) this.error = this.failure(error);
    } finally {
      if (this.current(generation)) this.pending = false;
    }
  }

  /** Activity is an advisory lease, independent from rendered message/read acknowledgement state. */
  setActivity(activity: PushActivity): void {
    this.activity = activity;
    if (!this.active || !this.binding || !this.enabled) return;
    const generation = this.generation;
    const binding = this.binding;
    this.activityTail = this.activityTail.then(async () => {
      if (!this.current(generation) || this.binding !== binding) return;
      try {
        await this.api.activity(binding.id, activity, this.controller.signal);
      } catch {
        // An absent or lost hint suppresses nothing after the server's thirty-second
        // expiry. Push never replaces the application's durable receipt mechanism.
      }
    });
  }

  /** Dispose route-owned work without cancelling the user's device-level consent. */
  dispose(): void {
    if (!this.active) return;
    this.active = false;
    this.generation++;
    this.controller.abort();
    this.enabled = false;
    this.pending = false;
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
    for (const cleanup of this.cleanups) cleanup();
    this.cleanups = [];
    // Any last foreground hint expires after thirty seconds; disposal creates no
    // read acknowledgement and leaves the capability for a same-session successor.
    this.binding = undefined;
    this.subscription = undefined;
    this.registration = undefined;
  }

  /** Revoke on logout/account change; serialize cleanup before a successor enrolls. */
  async stop(): Promise<void> {
    if (!this.active) return;
    const subscription = this.subscription;
    const registration = this.registration;
    this.revokeCapability = true;
    this.dispose();

    try {
      await serialized(() => this.clearBrowser(subscription, registration));
    } catch {
      // Logout/session revocation is authoritative even if optional browser cleanup
      // fails. Record this limitation without exposing endpoint/key material.
      this.error = '浏览器推送清理失败；账号权限仍由服务器校验。';
    }
  }

  private async bind(
    subscription: BrowserSubscription,
    generation: number,
  ): Promise<void> {
    const input = subscription.toJSON();
    if (!input.endpoint || !input.keys?.p256dh || !input.keys.auth) {
      throw new Error('浏览器推送订阅无效');
    }
    this.subscription = subscription;
    const binding = await this.api.register(
      {
        endpoint: input.endpoint,
        p256dh: input.keys.p256dh,
        auth: input.keys.auth,
      },
      this.controller.signal,
    );
    if (!this.current(generation)) return;
    this.binding = binding;
    this.enabled = true;
    this.setActivity({ ...this.activity, ...this.ports!.activity() });
  }

  private async clearBrowser(
    subscription: BrowserSubscription | undefined,
    registration: Registration | undefined,
  ): Promise<void> {
    let ownedRegistration = registration;
    try {
      // Optional server configuration can change while an older browser capability
      // remains installed. Logout still owns its local cleanup.
      ownedRegistration ??= await this.ports?.registration();
      const notifications = (await ownedRegistration?.getNotifications()) ?? [];
      for (const notification of notifications) notification.close();
    } finally {
      const ownedSubscription =
        subscription ??
        (await ownedRegistration?.pushManager.getSubscription());
      if (ownedSubscription) await ownedSubscription.unsubscribe();
    }
  }

  private key(): ArrayBuffer {
    const decoded = atob(this.publicKey!.replace(/-/g, '+').replace(/_/g, '/'));
    const bytes = new Uint8Array(decoded.length);
    for (let index = 0; index < decoded.length; index++) {
      bytes[index] = decoded.charCodeAt(index);
    }
    return bytes.buffer;
  }

  private current(generation: number): boolean {
    return this.active && this.generation === generation;
  }

  private failure(error: unknown): string {
    return error instanceof Error
      ? error.message
      : '后台推送设置失败，请稍后重试';
  }

  private listen(
    target: EventTarget,
    type: string,
    listener: EventListener,
  ): void {
    target.addEventListener(type, listener);
    this.cleanups.push(() => target.removeEventListener(type, listener));
  }
}
