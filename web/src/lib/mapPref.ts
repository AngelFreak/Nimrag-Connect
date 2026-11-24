import { useSyncExternalStore } from 'react';

// Per browser: draw activity routes on the interactive (WebGL) map, or as the route alone. Some
// GPU drivers fall over on WebGL; "route" keeps the activity page usable on such a machine.

export type MapChoice = 'interactive' | 'route';

const KEY = 'nimrag.map';
const EVENT = 'nimrag:map';

export function readMapChoice(): MapChoice {
  try {
    return window.localStorage.getItem(KEY) === 'route' ? 'route' : 'interactive';
  } catch {
    return 'interactive';
  }
}

export function setMapChoice(choice: MapChoice): void {
  try {
    if (choice === 'route') window.localStorage.setItem(KEY, 'route');
    else window.localStorage.removeItem(KEY);
  } catch {
    // Storage blocked: the choice lasts for this page only.
  }
  window.dispatchEvent(new Event(EVENT));
}

function subscribe(onChange: () => void): () => void {
  window.addEventListener(EVENT, onChange);
  window.addEventListener('storage', onChange);
  return () => {
    window.removeEventListener(EVENT, onChange);
    window.removeEventListener('storage', onChange);
  };
}

export function useMapChoice(): MapChoice {
  return useSyncExternalStore(subscribe, readMapChoice, () => 'interactive');
}
