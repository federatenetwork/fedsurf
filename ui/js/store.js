// Chrome-side state cache. Rust is the source of truth for tabs; this mirrors
// it (fed by fedsurf:// events) and drives a single render path.

export const state = {
  tabs: [],          // [{ id, url, title, favicon, loading }]
  activeId: null,
  collapsed: document.documentElement.classList.contains('collapsed'),
  platform: '',
};

const listeners = new Set();

export function subscribe(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function notify() {
  for (const fn of listeners) fn(state);
}

export function activeTab() {
  return state.tabs.find((t) => t.id === state.activeId) ?? null;
}

export function hostOf(url) {
  try {
    return new URL(url).host;
  } catch {
    return '';
  }
}
