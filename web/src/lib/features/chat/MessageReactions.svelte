<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';

  import type { Message, ReactionKind } from './models';
  import type { MessageActions } from './message-actions.svelte';

  const choices: ReadonlyArray<{
    kind: ReactionKind;
    emoji: string;
    label: string;
  }> = [
    { kind: 'like', emoji: '👍', label: '点赞' },
    { kind: 'love', emoji: '❤️', label: '喜欢' },
    { kind: 'laugh', emoji: '😂', label: '开心' },
    { kind: 'surprise', emoji: '😮', label: '惊讶' },
    { kind: 'sad', emoji: '😢', label: '难过' },
    { kind: 'celebrate', emoji: '🎉', label: '庆祝' },
  ];

  let { message, actions }: { message: Message; actions: MessageActions } =
    $props();

  let opened = $state(false);
  let trigger = $state<HTMLButtonElement | HTMLAnchorElement | null>(null);
  const pending = $derived(actions.reactionPending(message.id));

  async function choose(kind: ReactionKind | null) {
    opened = false;
    await actions.reaction(message.id, kind);
  }

  function escape(event: KeyboardEvent) {
    if (event.key === 'Escape' && opened) {
      opened = false;
      trigger?.focus();
    }
  }
</script>

<div class="reactions" role="group" aria-label="表情回应">
  {#each message.reactions ?? [] as summary (summary.kind)}
    {@const choice = choices.find((choice) => choice.kind === summary.kind)!}
    <Button
      type="button"
      size="xs"
      variant={summary.mine ? 'secondary' : 'ghost'}
      aria-label={`${choice.label}，${summary.count} 个回应`}
      aria-pressed={summary.mine}
      disabled={pending || message.withdrawn}
      onkeydown={escape}
      onclick={() => choose(summary.mine ? null : summary.kind)}
    >
      <span aria-hidden="true">{choice.emoji}</span>
      <span>{summary.count}</span>
    </Button>
  {/each}
  <Button
    type="button"
    size="xs"
    variant="ghost"
    bind:ref={trigger}
    aria-label="添加表情回应"
    aria-expanded={opened}
    disabled={pending || message.withdrawn}
    onclick={() => (opened = !opened)}
    onkeydown={escape}
  >
    <span aria-hidden="true">🙂</span>
  </Button>
  {#if opened}
    <div class="reaction-picker" role="group" aria-label="选择回应">
      {#each choices as choice (choice.kind)}
        {@const mine =
          message.reactions?.some(
            (summary) => summary.kind === choice.kind && summary.mine,
          ) ?? false}
        <Button
          type="button"
          size="icon-sm"
          variant={mine ? 'secondary' : 'ghost'}
          aria-label={choice.label}
          aria-pressed={mine}
          disabled={pending || message.withdrawn}
          onclick={() => choose(mine ? null : choice.kind)}
          onkeydown={escape}
        >
          <span aria-hidden="true">{choice.emoji}</span>
        </Button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .reactions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    margin-top: 0.25rem;
  }
  .reaction-picker {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    width: 100%;
  }
</style>
