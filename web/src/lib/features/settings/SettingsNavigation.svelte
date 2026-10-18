<script lang="ts">
  import { onMount } from 'svelte';

  const sections = [
    { id: 'settings-profile', label: '个人资料' },
    { id: 'settings-security', label: '账号安全' },
    { id: 'settings-preferences', label: '通知外观' },
    { id: 'settings-application', label: '应用' },
  ];

  let navigation: HTMLElement;
  let activeSection = $state(sections[0].id);
  let selectedAnchor: { id: string; scrollTop?: number } | undefined;
  let scheduleUpdate: (() => void) | undefined;

  function selectSection(id: string) {
    selectedAnchor = { id };
    activeSection = id;
    scheduleUpdate?.();
  }

  onMount(() => {
    const container = navigation.closest<HTMLElement>('.app-content');
    if (!container) return;

    const viewport = container;
    const settingsPage = navigation.closest<HTMLElement>('.settings-page');
    const headings = sections.map((section) =>
      document.getElementById(section.id),
    );
    let frame = 0;

    function updateSection() {
      frame = 0;
      // Short groups can share the same clamped scroll position near the end.
      // Keep an explicitly chosen anchor selected until the viewport moves again.
      if (selectedAnchor) {
        selectedAnchor.scrollTop ??= viewport.scrollTop;
        if (Math.abs(selectedAnchor.scrollTop - viewport.scrollTop) < 1) {
          activeSection = selectedAnchor.id;
          return;
        }
        selectedAnchor = undefined;
      }

      const readingLine = Math.ceil(navigation.getBoundingClientRect().bottom);
      let current = sections[0].id;
      for (const heading of headings) {
        if (heading && heading.getBoundingClientRect().top <= readingLine) {
          current = heading.id;
        }
      }
      activeSection = current;
    }

    const requestUpdate = () => {
      if (!frame) frame = requestAnimationFrame(updateSection);
    };
    scheduleUpdate = requestUpdate;

    function updateNavigationLayout() {
      // Anchor offsets and scroll tracking share the actual sticky surface height,
      // including its bottom spacing, so preceding cards stay behind the surface.
      settingsPage?.style.setProperty(
        '--settings-navigation-height',
        `${navigation.getBoundingClientRect().height}px`,
      );
      requestUpdate();
    }

    // The app owns scrolling. Reading the four headings once per frame keeps
    // manual scroll and anchor navigation in sync without changing browser history.
    viewport.addEventListener('scroll', requestUpdate, { passive: true });
    const resizeObserver = new ResizeObserver(updateNavigationLayout);
    resizeObserver.observe(navigation);
    const content = settingsPage?.querySelector('.settings-content');
    if (content) resizeObserver.observe(content);
    updateNavigationLayout();

    return () => {
      viewport.removeEventListener('scroll', requestUpdate);
      resizeObserver.disconnect();
      cancelAnimationFrame(frame);
      settingsPage?.style.removeProperty('--settings-navigation-height');
      scheduleUpdate = undefined;
    };
  });
</script>

<nav
  class="settings-navigation-bar"
  aria-label="设置分类"
  bind:this={navigation}
>
  <div class="settings-container">
    <div class="settings-navigation">
      {#each sections as section (section.id)}
        <a
          href={`#${section.id}`}
          onclick={() => selectSection(section.id)}
          aria-current={activeSection === section.id ? 'location' : undefined}
        >
          {section.label}
        </a>
      {/each}
    </div>
  </div>
</nav>
