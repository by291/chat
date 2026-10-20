import { getContext, setContext } from 'svelte';
import type { ContactsState } from './contacts.svelte';

const key = Symbol('contacts');

export type ContactCategory = 'friends' | 'incoming' | 'outgoing';

/** The authenticated application layout creates and tears down this owner. */
class ContactsContext {
  owner = $state<ContactsState | null>(null);
  view = $state<{
    category: ContactCategory;
    selectedId: string | null;
    scrollTop: number;
  }>({ category: 'friends', selectedId: null, scrollTop: 0 });
}

export function provideContacts() {
  const context = new ContactsContext();
  setContext(key, context);
  return context;
}
export function useContacts() {
  return getContext<ContactsContext>(key);
}
