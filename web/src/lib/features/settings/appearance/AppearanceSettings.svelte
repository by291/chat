<script lang="ts">
  import * as Card from '#lib/components/ui/card/index.js';
  import * as NativeSelect from '#lib/components/ui/native-select/index.js';
  import { Label } from '#lib/components/ui/label/index.js';

  import { theme, type Theme } from './theme.svelte';
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="界面外观">
  <div class="section-heading">
    <h3>界面外观</h3>
  </div>
  <Label class="form-label" for="theme">外观</Label>
  <NativeSelect.Root
    class="w-full"
    id="theme"
    value={theme.value}
    onchange={(e) => theme.set(e.currentTarget.value as Theme)}
  >
    <NativeSelect.Option value="system">跟随系统</NativeSelect.Option>
    <NativeSelect.Option value="light">浅色</NativeSelect.Option>
    <NativeSelect.Option value="dark">深色</NativeSelect.Option>
  </NativeSelect.Root>
</Card.Root>
