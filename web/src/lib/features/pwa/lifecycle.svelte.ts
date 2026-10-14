type WaitingWorker = Pick<ServiceWorker, 'postMessage'>;
type Registration = EventTarget & {
  waiting: WaitingWorker | null;
  installing: (EventTarget & { state: ServiceWorkerState }) | null;
};
type WorkerContainer = EventTarget & {
  getRegistration(): Promise<Registration | undefined>;
  ready: Promise<Registration>;
};
type PwaPorts = {
  events: EventTarget;
  serviceWorker?: WorkerContainer;
  standalone(): boolean;
  reload(): void;
};
type DeferredPrompt = Event & {
  prompt(): Promise<void>;
  userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }>;
};
type PendingWork = {
  hasDraft: boolean;
  hasUpload: boolean;
  hasSubmission: boolean;
};

function browserPorts(): PwaPorts | undefined {
  if (typeof window === 'undefined') return undefined;
  return {
    events: window,
    serviceWorker: window.isSecureContext ? navigator.serviceWorker : undefined,
    standalone: () =>
      window.matchMedia('(display-mode: standalone)').matches ||
      (navigator as Navigator & { standalone?: boolean }).standalone === true,
    reload: () => window.location.reload(),
  };
}

/** Root browser owner for install/update hints; it never owns authenticated records or drafts. */
export class PwaLifecycle {
  installAvailable = $state(false);
  installed = $state(false);
  installPending = $state(false);
  updateAvailable = $state(false);
  updatePending = $state(false);
  confirmationRequired = $state(false);
  error = $state('');
  private active = false;
  private generation = 0;
  private cleanups: Array<() => void> = [];
  private registration?: Registration;
  private installing?: Registration['installing'];
  private waiting?: WaitingWorker;
  private prompt?: DeferredPrompt;
  private handoffRequested = false;
  private handoffDeadline?: ReturnType<typeof setTimeout>;

  constructor(private ports?: PwaPorts) {}

  /** Attach browser listeners once; a late registration cannot revive a disposed owner. */
  async start(): Promise<void> {
    if (this.active) return;
    const ports = this.ports ?? browserPorts();
    if (!ports) return;
    this.ports = ports;
    this.active = true;
    const generation = ++this.generation;
    this.installed = ports.standalone();
    this.listen(ports.events, 'beforeinstallprompt', (event) => {
      const prompt = event as DeferredPrompt;
      if (typeof prompt.prompt !== 'function' || !prompt.userChoice) return;
      event.preventDefault();
      this.prompt = prompt;
      this.installAvailable = true;
    });
    this.listen(ports.events, 'appinstalled', () => {
      this.installed = true;
      this.installAvailable = false;
      this.prompt = undefined;
    });
    const container = ports.serviceWorker;
    if (!container) return;
    this.listen(container, 'controllerchange', () => {
      if (this.handoffRequested) {
        this.handoffRequested = false;
        this.clearDeadline();
        ports.reload();
      } else {
        // Another tab can update its worker. That event is never consent to
        // discard this tab's drafts or recreate its authenticated lifetime.
        void this.refresh(generation);
      }
    });
    void container.ready
      .then((registration) => {
        if (this.current(generation)) this.observe(registration);
      })
      .catch(() => {
        // Unsupported or failed registration leaves manual installation guidance usable.
      });
    await this.refresh(generation);
  }

  /** Release listeners, prompt references and deadlines without activating or reloading. */
  stop() {
    this.active = false;
    this.generation++;
    for (const cleanup of this.cleanups.splice(0)) cleanup();
    this.clearDeadline();
    this.registration = undefined;
    this.installing = undefined;
    this.waiting = undefined;
    this.prompt = undefined;
    this.handoffRequested = false;
    this.installAvailable = false;
    this.installPending = false;
    this.updateAvailable = false;
    this.updatePending = false;
    this.confirmationRequired = false;
  }

  async install(): Promise<void> {
    const prompt = this.prompt;
    if (!this.active || !prompt || this.installPending) return;
    const generation = this.generation;
    this.installPending = true;
    this.error = '';

    try {
      await prompt.prompt();
      await prompt.userChoice;
    } catch {
      if (this.current(generation))
        this.error = '暂时无法安装，请使用浏览器菜单重试';
    } finally {
      if (this.current(generation)) {
        this.prompt = undefined;
        this.installAvailable = false;
        this.installPending = false;
      }
    }
  }

  /** A user action can request handoff; pending work first requires the warning's confirmation. */
  async requestUpdate(work: PendingWork): Promise<void> {
    if (!this.active || !this.waiting || this.updatePending) return;
    this.error = '';
    this.confirmationRequired =
      work.hasDraft || work.hasUpload || work.hasSubmission;
    if (!this.confirmationRequired) this.activate();
  }

  cancelUpdate() {
    this.confirmationRequired = false;
  }

  confirmUpdate() {
    if (!this.confirmationRequired) return;
    this.confirmationRequired = false;
    this.activate();
  }

  private activate() {
    if (!this.active || !this.waiting || this.updatePending) return;
    this.updatePending = true;
    this.handoffRequested = true;
    this.waiting.postMessage({ type: 'activate-update' });
    this.handoffDeadline = setTimeout(() => {
      this.handoffRequested = false;
      this.updatePending = false;
      this.error = '更新暂未完成，请稍后重试';
    }, 20_000);
  }

  private clearDeadline() {
    clearTimeout(this.handoffDeadline);
    this.handoffDeadline = undefined;
  }

  private current(generation: number) {
    return this.active && generation === this.generation;
  }

  private listen(target: EventTarget, type: string, listener: EventListener) {
    target.addEventListener(type, listener);
    this.cleanups.push(() => target.removeEventListener(type, listener));
  }

  private async refresh(generation: number) {
    try {
      const registration = await this.ports?.serviceWorker?.getRegistration();
      if (registration && this.current(generation)) this.observe(registration);
    } catch {
      // Registration is optional and retried on ready/controller changes; no durable work is lost.
    }
  }

  private observe(registration: Registration) {
    this.waiting = registration.waiting ?? undefined;
    this.updateAvailable = !!this.waiting;
    if (this.registration !== registration) {
      this.registration = registration;
      this.listen(registration, 'updatefound', () =>
        this.observe(registration),
      );
    }
    if (
      registration.installing &&
      registration.installing !== this.installing
    ) {
      this.installing = registration.installing;
      this.listen(registration.installing, 'statechange', () =>
        this.observe(registration),
      );
    }
  }
}

// This SPA disables SSR. Browser listeners start only from the root onMount;
// revisit per-request ownership before enabling server rendering.
export const pwa = new PwaLifecycle();
