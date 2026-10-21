export type Theme = 'system' | 'light' | 'dark';

/** Browser appearance state; preference storage contains no session-owned records. */
class ThemeState {
  value = $state<Theme>('dark');

  private media: MediaQueryList | undefined;

  init() {
    this.media = matchMedia('(prefers-color-scheme: dark)');
    let saved: string | null = null;

    try {
      saved = localStorage.getItem('chat.theme');
    } catch {
      // Storage may be disabled; the default dark theme remains usable.
    }

    if (saved === 'light' || saved === 'dark' || saved === 'system') {
      this.value = saved;
    }

    this.apply();
    const update = () => this.apply();
    this.media.addEventListener('change', update);

    return () => this.media?.removeEventListener('change', update);
  }

  set(value: Theme) {
    this.value = value;

    try {
      localStorage.setItem('chat.theme', value);
    } catch {
      // Apply the selected theme for this page even when persistence is unavailable.
    }

    this.apply();
  }

  private apply() {
    const dark =
      this.value === 'dark' || (this.value === 'system' && this.media?.matches);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    document.documentElement.style.colorScheme = dark ? 'dark' : 'light';
  }
}

export const theme = new ThemeState();
