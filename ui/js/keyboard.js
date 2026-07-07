// Chrome-webview shortcut mirror. Pages get the same combos via the injected
// script (see src-tauri/src/ua.rs); macOS menu accelerators cover both.

import { state } from './store.js';
import { toggleCollapsed, newTab } from './sidebar.js';
import { focusAddress } from './toolbar.js';

const { invoke } = window.__TAURI__.core;

const isMac = navigator.platform.toUpperCase().includes('MAC');

function cycle(delta) {
  const { tabs, activeId } = state;
  if (!tabs.length) return;
  const i = Math.max(0, tabs.findIndex((t) => t.id === activeId));
  const next = tabs[(i + delta + tabs.length) % tabs.length];
  invoke('activate_tab', { id: next.id });
}

function jump(n) {
  const { tabs } = state;
  if (!tabs.length) return;
  const tab = n >= 9 ? tabs[tabs.length - 1] : tabs[n - 1];
  if (tab) invoke('activate_tab', { id: tab.id });
}

export function initKeyboard() {
  window.addEventListener('keydown', (e) => {
    let handled = true;
    if (e.ctrlKey && !e.metaKey && !e.altKey && e.key === 'Tab') {
      cycle(e.shiftKey ? -1 : 1);
    } else {
      const mod = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      if (!mod || e.altKey || e.shiftKey) return;
      const k = e.key.toLowerCase();
      if (k === 't') newTab();
      else if (k === 'w') state.activeId != null && invoke('close_tab', { id: state.activeId });
      else if (k === 'l') focusAddress();
      else if (k === 'b') toggleCollapsed();
      else if (k === 'r') invoke('reload');
      else if (k === '[') invoke('go_back');
      else if (k === ']') invoke('go_forward');
      else if (k >= '1' && k <= '9') jump(Number(k));
      else handled = false;
    }
    if (handled) e.preventDefault();
  });
}
