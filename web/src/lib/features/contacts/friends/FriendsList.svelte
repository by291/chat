<script lang="ts">
  import ContactRow from '../ContactRow.svelte';
  import type { FriendsPage } from '../api';

  let {
    friends,
    selectedId,
    loading,
    searching,
    term,
    onselect,
  }: {
    friends: FriendsPage['items'];
    selectedId: string | null;
    loading: boolean;
    searching: boolean;
    term: string;
    onselect: (id: string) => void;
  } = $props();
</script>

{#each friends as friend (friend.user.id)}
  <ContactRow user={friend.user} id={friend.user.id} {selectedId} {onselect} />
{:else}
  {#if !loading && !searching}
    <li class="contact-empty">
      {term ? '没有找到匹配的好友' : '还没有好友，先添加一位吧。'}
    </li>
  {/if}
{/each}
