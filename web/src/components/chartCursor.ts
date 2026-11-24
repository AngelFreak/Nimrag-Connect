import { useState, useSyncExternalStore } from 'react';

/**
 * Where the pointer is on a group of charts that share their samples: the index of the nearest
 * sample, or `null` when it is on none. Charts write it, readouts subscribe, and the charts around
 * them do not re-render on every move.
 */
export class CursorStore {
  private current: number | null = null;
  private readonly listeners = new Set<() => void>();

  get index(): number | null {
    return this.current;
  }

  set = (index: number | null): void => {
    if (index === this.current) return;
    this.current = index;
    this.listeners.forEach((l) => l());
  };

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
}

/** One store for the life of the component. */
export function useCursorStore(): CursorStore {
  return useState(() => new CursorStore())[0];
}

/** The sample under the pointer; re-renders only the component that asks. */
export function useCursorIndex(store: CursorStore): number | null {
  return useSyncExternalStore(store.subscribe, () => store.index, () => null);
}
