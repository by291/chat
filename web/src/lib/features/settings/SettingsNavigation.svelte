<script lang="ts">
  import { tick } from 'svelte';
  import { goto } from '$app/navigation';
  import { ShieldCheck, Bell, Smartphone } from '@lucide/svelte';

  let { active }: { active: string } = $props();
  const sections = [
    { id: 'settings-security', label: '账号安全', icon: ShieldCheck },
    { id: 'settings-preferences', label: '通知外观', icon: Bell },
    { id: 'settings-application', label: '应用', icon: Smartphone },
  ];
</script>

<nav class="settings-navigation" aria-label="设置分类">
  {#each sections as section (section.id)}
    <a
      href={`#${section.id}`}
      aria-current={active === section.id ? 'page' : undefined}
      onclick={async (event) => {
        event.preventDefault();
        await goto(`#${section.id}`, { reset: false });
        await tick();
        document.getElementById(section.id)?.focus({ preventScroll: true });
      }}
    >
      <section.icon size={19} aria-hidden="true" />
      <span>{section.label}</span>
    </a>
  {/each}
</nav>
