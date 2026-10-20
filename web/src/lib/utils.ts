import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

/** Merge component defaults with caller overrides without conflicting utilities. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Keep fallback avatar colors stable across lists, profiles and chat headers. */
export function avatarTone(identity: string): number {
  return (
    Array.from(identity).reduce(
      (hash, character) => (hash * 31 + character.charCodeAt(0)) >>> 0,
      0,
    ) % 4
  );
}

export type WithElementRef<T, U extends HTMLElement = HTMLElement> = T & {
  ref?: U | null;
};

export type WithoutChild<T> = T extends { child?: unknown }
  ? Omit<T, 'child'>
  : T;
export type WithoutChildren<T> = T extends { children?: unknown }
  ? Omit<T, 'children'>
  : T;
export type WithoutChildrenOrChild<T> = WithoutChildren<WithoutChild<T>>;
