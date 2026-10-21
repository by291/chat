<script lang="ts">
  import ContactRow from '../ContactRow.svelte';
  import type { ContactRequest } from './model';

  let {
    requests,
    selectedId,
    loading,
    searching,
    term,
    onselect,
  }: {
    requests: ContactRequest[];
    selectedId: string | null;
    loading: boolean;
    searching: boolean;
    term: string;
    onselect: (id: string) => void;
  } = $props();
</script>

{#each requests as request (request.id)}
  <ContactRow
    user={request.user}
    id={request.id}
    {selectedId}
    {onselect}
    description={request.direction === 'incoming'
      ? '请求添加你 · 待你确认'
      : '你发出的申请 · 等待同意'}
  />
{:else}
  {#if !loading && !searching}
    <li class="contact-empty">
      {term ? '没有找到匹配的申请' : '暂无好友申请'}
    </li>
  {/if}
{/each}
