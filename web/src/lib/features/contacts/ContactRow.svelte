<script lang="ts">
  import { Button } from '#lib/components/ui/button/index.js';
  import * as Avatar from '#lib/components/ui/avatar/index.js';
  import { avatarTone } from '#lib/utils.js';

  import type { User } from './api';

  let {
    user,
    id,
    description,
    selectedId,
    onselect,
  }: {
    user: User;
    id: string;
    description?: string;
    selectedId: string | null;
    onselect: (id: string) => void;
  } = $props();
</script>

<li>
  <Button
    variant="ghost"
    class="contact-entry"
    aria-label={`${user.display_name} (@${user.username})${description ? `，${description}` : ''}`}
    aria-pressed={selectedId === id}
    data-contact-id={id}
    onclick={() => onselect(id)}
  >
    <Avatar.Root data-tone={avatarTone(user.id)}>
      <Avatar.Fallback>
        {Array.from(user.display_name)[0] ?? '…'}
      </Avatar.Fallback>
    </Avatar.Root>
    <span class="contact-copy">
      <strong>{user.display_name}</strong>
      <small>{description ?? `@${user.username}`}</small>
    </span>
  </Button>
</li>
