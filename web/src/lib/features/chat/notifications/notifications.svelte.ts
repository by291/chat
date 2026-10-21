import type { Conversation, Message } from '../models';
import { conversationName } from '../models';

const MAX_SEEN_MESSAGES = 200;
const MAX_VISIBLE_ALERTS = 5;
const SOUND_DURATION_SECONDS = 0.15;

export type Alert = {
  messageId: string;
  conversationId: string;
  title: string;
};

/** Pure open-page policy; neither notification delivery nor dismissal marks a read. */
export function shouldNotify(input: {
  fresh: boolean;
  incoming: boolean;
  withdrawn: boolean;
  muted: boolean;
  focusedVisibleCurrent: boolean;
}) {
  return (
    input.fresh &&
    input.incoming &&
    !input.withdrawn &&
    !input.muted &&
    !input.focusedVisibleCurrent
  );
}

/** Browser capabilities are created through user gestures and closed on logout.
 * In-app feedback always remains available when sound or system alerts fail.
 */
export class ChatNotifications {
  items = $state<Alert[]>([]);
  sound = $state(false);
  system = $state(false);
  permission = $state<NotificationPermission | 'unsupported'>('unsupported');
  feedback = $state('');

  private active = true;
  private seen = new Set<string>();
  private audio: AudioContext | undefined;
  private alerts = new Set<Notification>();
  private openConversation: ((id: string) => void) | undefined;

  constructor(
    private options: {
      userId: string;
      selectedId: () => string | null;
      visible: () => boolean;
    },
  ) {}

  mount(openConversation?: (id: string) => void) {
    this.openConversation = openConversation;
    if (typeof Notification !== 'undefined') {
      this.permission = Notification.permission;
    }
  }

  applied(
    record: Message,
    conversation: Conversation | undefined,
    fresh: boolean,
  ) {
    if (!this.active || !conversation || !fresh || this.seen.has(record.id)) {
      return;
    }

    // Consume an exact live candidate even when focus or mute suppresses it.
    // A duplicate publication after leaving/unmuting must not revive that alert;
    // replay without a live candidate stays eligible for its later exact hint.
    this.seen.add(record.id);
    if (this.seen.size > MAX_SEEN_MESSAGES) {
      this.seen.delete(this.seen.values().next().value!);
    }

    if (
      !shouldNotify({
        fresh,
        incoming: record.sender_id !== this.options.userId,
        withdrawn: record.withdrawn,
        muted: conversation.preferences.muted,
        focusedVisibleCurrent:
          this.options.selectedId() === record.conversation_id &&
          this.options.visible(),
      })
    ) {
      return;
    }

    const alert = {
      messageId: record.id,
      conversationId: record.conversation_id,
      title: conversationName(conversation),
    };
    this.items = [...this.items, alert].slice(-MAX_VISIBLE_ALERTS);
    if (this.sound && this.audio) void this.playSound();
    if (this.system && this.permission === 'granted') {
      try {
        const notification = new Notification(`${alert.title} · 新消息`, {
          body: '打开聊天查看',
          tag: record.id,
        });
        this.alerts.add(notification);
        notification.onclose = () => this.alerts.delete(notification);
        notification.onclick = () => {
          if (!this.active) return;
          window.focus();
          this.openConversation?.(record.conversation_id);
          this.dismiss(record.id);
          notification.close();
        };
        // System notifications carry no sender text or message body. Browsers may
        // keep them beyond their page lifetime unless they are explicitly closed.
        if (this.alerts.size > MAX_VISIBLE_ALERTS) {
          const oldest = this.alerts.values().next().value!;
          oldest.close();
          this.alerts.delete(oldest);
        }
      } catch {
        this.feedback = '系统通知不可用，仍会在页面内提醒';
      }
    }
  }

  dismiss(messageId: string) {
    this.items = this.items.filter((item) => item.messageId !== messageId);
  }

  async toggleSound() {
    if (this.sound) {
      this.sound = false;
      return;
    }
    try {
      if (typeof AudioContext === 'undefined') throw new Error('unsupported');
      this.audio ??= new AudioContext();
      await this.audio.resume();
      if (this.active) {
        this.sound = true;
        this.feedback = '';
      }
    } catch {
      if (this.active) this.feedback = '提示音不可用，仍会在页面内提醒';
    }
  }

  async toggleSystem() {
    if (this.system) {
      this.system = false;
      return;
    }
    if (typeof Notification === 'undefined') {
      this.feedback = '此浏览器不支持系统通知，仍会在页面内提醒';
      return;
    }
    try {
      // This method is called only by the explicit settings button click.
      const permission =
        Notification.permission === 'default'
          ? await Notification.requestPermission()
          : Notification.permission;
      if (!this.active) return;
      this.permission = permission;
      this.system = permission === 'granted';
      this.feedback = this.system ? '' : '系统通知权限未开启，仍会在页面内提醒';
    } catch {
      if (this.active) this.feedback = '系统通知不可用，仍会在页面内提醒';
    }
  }

  private async playSound() {
    try {
      if (!this.audio || this.audio.state !== 'running') return;
      const oscillator = this.audio.createOscillator();
      const gain = this.audio.createGain();
      oscillator.frequency.value = 660;
      gain.gain.setValueAtTime(0.06, this.audio.currentTime);
      gain.gain.exponentialRampToValueAtTime(
        0.001,
        this.audio.currentTime + SOUND_DURATION_SECONDS,
      );
      oscillator.connect(gain);
      gain.connect(this.audio.destination);
      oscillator.start();
      oscillator.stop(this.audio.currentTime + SOUND_DURATION_SECONDS);
      oscillator.onended = () => {
        oscillator.disconnect();
        gain.disconnect();
      };
    } catch {
      if (this.active) this.feedback = '提示音暂不可用';
    }
  }

  stop() {
    this.active = false;
    this.openConversation = undefined;
    this.seen.clear();
    this.items = [];
    this.sound = false;
    this.system = false;
    for (const notification of this.alerts) notification.close();
    this.alerts.clear();
    // Audio resources are ephemeral; closing can reject when a browser is shutting down.
    void this.audio?.close().catch(() => {});
    this.audio = undefined;
  }
}
