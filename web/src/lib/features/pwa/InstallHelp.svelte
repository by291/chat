<script lang="ts">
  import * as Card from '#lib/components/ui/card/index.js';
  import { Button } from '#lib/components/ui/button/index.js';

  import { pwa } from './lifecycle.svelte';
</script>

<Card.Root class="settings-card gap-0 p-6" role="region" aria-label="安装应用">
  <div class="section-heading">
    <h2>安装应用</h2>
    <p class="muted">将 Chat 放到桌面或主屏幕，方便下次打开。</p>
  </div>
  {#if pwa.installed}
    <p role="status">已作为应用安装。</p>
  {:else if pwa.installAvailable}
    <Button
      type="button"
      disabled={pwa.installPending}
      onclick={() => pwa.install()}
    >
      {pwa.installPending ? '正在安装…' : '安装 Chat'}
    </Button>
  {:else}
    <p>
      在支持的浏览器菜单中选择“安装应用”或“添加到主屏幕”。iPhone／iPad 请在
      Safari 分享菜单中选择“添加到主屏幕”。
    </p>
  {/if}
  <p class="muted">离线时显示连接提示；聊天记录需要联网读取。</p>
  {#if pwa.error}
    <p role="status">{pwa.error}</p>
  {/if}
</Card.Root>
