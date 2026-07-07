// Boot: wire Tauri events into the store, then hand Rust the persisted
// sidebar width and take the authoritative tab snapshot back.

import { state, subscribe, notify, hostOf } from './store.js';
import * as sidebar from './sidebar.js';
import * as toolbar from './toolbar.js';
import { initKeyboard } from './keyboard.js';

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const RECENT_KEY = 'fedsurf.recent';

toolbar.init();
sidebar.init();
initKeyboard();
subscribe(sidebar.render);
subscribe(toolbar.render);

// Listeners are registered before frontend_ready, so no event can slip
// between the snapshot and the live stream.
await listen('fedsurf://tab-created', (e) => {
  const { id, url, index, loading = true } = e.payload;
  if (!state.tabs.some((t) => t.id === id)) {
    state.tabs.splice(index, 0, { id, url, title: '', favicon: '', loading: Boolean(loading) });
  }
  notify();
});
await listen('fedsurf://tab-updated', (e) => {
  const tab = state.tabs.find((t) => t.id === e.payload.id);
  if (tab) {
    Object.assign(tab, e.payload);
    rememberRecent(tab);
    notify();
  }
});
await listen('fedsurf://tab-activated', (e) => {
  state.activeId = e.payload;
  notify();
});
await listen('fedsurf://tab-closed', (e) => {
  state.tabs = state.tabs.filter((t) => t.id !== e.payload);
  notify();
});
await listen('fedsurf://toggle-sidebar', () => sidebar.toggleCollapsed());
await listen('fedsurf://focus-address', () => toolbar.focusAddress());

const snapshot = await invoke('frontend_ready', { sidebarPx: sidebar.sidebarPx() });
state.tabs = snapshot.tabs;
state.activeId = snapshot.active;
state.platform = snapshot.platform;
document.body.dataset.platform = snapshot.platform;
state.tabs.forEach(rememberRecent);
notify();

window.addEventListener('error', (e) => {
  invoke('frontend_log', { msg: `js-error: ${e.message} @ ${e.filename}:${e.lineno}` }).catch(() => {});
});
invoke('frontend_log', {
  msg: JSON.stringify({
    cls: document.documentElement.className,
    viewport: `${window.innerWidth}x${window.innerHeight}`,
    dpr: window.devicePixelRatio,
    sidebarW: getComputedStyle(document.getElementById('sidebar')).width,
    varW: getComputedStyle(document.documentElement).getPropertyValue('--sidebar-w'),
    platform: state.platform,
  }),
}).catch(() => {});

function rememberRecent(tab) {
  if (!tab || !tab.url || tab.loading) return;
  if (!/^(fed|https?):\/\//.test(tab.url)) return;
  let current = [];
  try {
    const parsed = JSON.parse(localStorage.getItem(RECENT_KEY) || '[]');
    if (Array.isArray(parsed)) current = parsed;
  } catch { /* ignore malformed history */ }
  const item = {
    url: tab.url,
    title: tab.title || hostOf(tab.url) || tab.url,
    at: Date.now(),
  };
  const next = [
    item,
    ...current.filter((entry) => entry && entry.url !== item.url),
  ].slice(0, 12);
  localStorage.setItem(RECENT_KEY, JSON.stringify(next));
}
