// The vertical tab list: keyed DOM rendering (nodes are reused and reordered,
// never wiped), enter/exit animations, favicon/letter-tile/spinner slot,
// roving-tabindex keyboard nav, collapse toggle.

import { state, notify, hostOf } from './store.js';
import { initReorder, clickWasDrag } from './reorder.js';

const { invoke } = window.__TAURI__.core;

const listEl = document.getElementById('tab-list');
const toggleBtn = document.getElementById('toggle-sidebar');
const nodes = new Map(); // id -> row element

const EXPANDED_PX = 240;
const COLLAPSED_PX = 52;
const SIDEBAR_ANIM_MS = 190;
const TILE_COLORS = ['var(--teal)', 'var(--navy)', 'var(--olive)', 'var(--terracotta)', 'var(--amber)'];
const CLOSE_SVG =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6l-12 12"/><path d="M6 6l12 12"/></svg>';

let lastActiveId = null;
let sidebarAnim = 0;

/* ---- helpers ------------------------------------------------------------ */

function tileFor(url) {
  const host = hostOf(url);
  if (!host) return { letter: '+', color: 'var(--accent)' };
  let hash = 0;
  for (let i = 0; i < host.length; i++) hash = (hash * 31 + host.charCodeAt(i)) >>> 0;
  return { letter: host[0], color: TILE_COLORS[hash % TILE_COLORS.length] };
}

function titleFor(tab) {
  if (tab.title) return { text: tab.title, faded: false };
  if (tab.loading) return { text: 'Loading…', faded: true };
  const host = hostOf(tab.url);
  return host ? { text: host, faded: false } : { text: 'New Tab', faded: true };
}

function faviconFor(tab) {
  if (tab.favicon) return tab.favicon;
  try {
    const u = new URL(tab.url);
    if (['http:', 'https:', 'fed:'].includes(u.protocol) && u.host) {
      return `${u.protocol}//${u.host}/favicon.ico`;
    }
  } catch { /* not a URL yet */ }
  return '';
}

function liveRows() {
  return [...listEl.querySelectorAll('.tab:not(.exiting)')];
}

/* ---- row construction ---------------------------------------------------- */

function createNode(tab) {
  const row = document.createElement('div');
  row.className = 'tab';
  row.setAttribute('role', 'tab');
  row.dataset.id = String(tab.id);

  const icon = document.createElement('span');
  icon.className = 'tab-icon';
  const img = document.createElement('img');
  img.className = 'favicon';
  img.alt = '';
  img.draggable = false;
  const tile = document.createElement('span');
  tile.className = 'tile';
  const spinner = document.createElement('span');
  spinner.className = 'spinner';
  spinner.setAttribute('aria-hidden', 'true');
  icon.append(img, tile, spinner);

  const title = document.createElement('span');
  title.className = 'tab-title';

  const close = document.createElement('button');
  close.className = 'tab-close';
  close.tabIndex = -1;
  close.innerHTML = CLOSE_SVG; // static markup, no user content

  row.append(icon, title, close);

  img.addEventListener('error', () => {
    row._brokenIcon = img.src;
    syncIconMode(row);
  });
  img.addEventListener('load', () => {
    if (row._brokenIcon === img.src) row._brokenIcon = null;
    syncIconMode(row);
  });

  row.addEventListener('click', (e) => {
    if (clickWasDrag() || e.target.closest('.tab-close')) return;
    invoke('activate_tab', { id: tab.id });
  });
  row.addEventListener('auxclick', (e) => {
    if (e.button === 1) invoke('close_tab', { id: tab.id });
  });
  close.addEventListener('click', () => {
    invoke('close_tab', { id: tab.id });
  });

  return row;
}

function syncIconMode(row) {
  const tab = state.tabs.find((t) => t.id === Number(row.dataset.id));
  if (!tab) return;
  const img = row.querySelector('.favicon');
  if (tab.loading) {
    row.dataset.icon = 'spinner';
    row.setAttribute('aria-busy', 'true');
    return;
  }
  row.removeAttribute('aria-busy');
  const src = faviconFor(tab);
  if (src && row._brokenIcon !== src) {
    if (img.getAttribute('src') !== src) img.src = src;
    row.dataset.icon = 'favicon';
  } else {
    row.dataset.icon = 'tile';
  }
}

function updateNode(row, tab) {
  const { text, faded } = titleFor(tab);
  const titleEl = row.querySelector('.tab-title');
  if (titleEl.textContent !== text) titleEl.textContent = text;
  titleEl.classList.toggle('faded', faded);

  const { letter, color } = tileFor(tab.url);
  const tile = row.querySelector('.tile');
  tile.textContent = letter;
  tile.style.background = color;

  const selected = tab.id === state.activeId;
  row.setAttribute('aria-selected', String(selected));
  row.tabIndex = selected ? 0 : -1;
  row.setAttribute('aria-label', text);
  row.title = state.collapsed ? (tab.url ? `${text}\n${tab.url}` : text) : '';
  row.querySelector('.tab-close').setAttribute('aria-label', `Close ${text}`);

  syncIconMode(row);
}

/* ---- render -------------------------------------------------------------- */

export function render() {
  let prev = null;
  for (const tab of state.tabs) {
    let node = nodes.get(tab.id);
    const anchor = prev ? prev.nextSibling : listEl.firstChild;
    if (!node) {
      node = createNode(tab);
      nodes.set(tab.id, node);
      node.classList.add('entering');
      listEl.insertBefore(node, anchor);
      requestAnimationFrame(() =>
        requestAnimationFrame(() => node.classList.remove('entering'))
      );
    } else if (anchor !== node) {
      listEl.insertBefore(node, anchor);
    }
    updateNode(node, tab);
    prev = node;
  }
  for (const [id, node] of nodes) {
    if (!state.tabs.some((t) => t.id === id)) {
      nodes.delete(id);
      node.classList.add('exiting');
      node.setAttribute('aria-hidden', 'true');
      const remove = () => node.remove();
      node.addEventListener('transitionend', remove, { once: true });
      setTimeout(remove, 250); // reduced-motion fallback
    }
  }
  if (state.activeId !== lastActiveId) {
    lastActiveId = state.activeId;
    nodes.get(state.activeId)?.scrollIntoView({ block: 'nearest' });
  }
  updateFade();
}

function updateFade() {
  const top = listEl.scrollTop > 4;
  const bottom = listEl.scrollTop < listEl.scrollHeight - listEl.clientHeight - 4;
  listEl.dataset.fade = top && bottom ? 'both' : top ? 'top' : bottom ? 'bottom' : 'none';
}

/* ---- collapse toggle ------------------------------------------------------ */

export function toggleCollapsed() {
  const sidebarEl = document.getElementById('sidebar');
  const fromPx = parseFloat(getComputedStyle(sidebarEl).width) || sidebarPx();
  const root = document.documentElement;
  cancelAnimationFrame(sidebarAnim);
  root.classList.add('sidebar-resizing');
  root.style.setProperty('--sidebar-w', `${fromPx.toFixed(2)}px`);

  state.collapsed = !state.collapsed;
  root.classList.toggle('collapsed', state.collapsed);
  localStorage.setItem('fedsurf.sidebar.collapsed', state.collapsed ? '1' : '0');
  toggleBtn.setAttribute('aria-expanded', String(!state.collapsed));
  const label = state.collapsed ? 'Expand sidebar' : 'Collapse sidebar';
  toggleBtn.setAttribute('aria-label', label);
  toggleBtn.title = label;
  const px = state.collapsed ? COLLAPSED_PX : EXPANDED_PX;
  animateSidebarWidth(fromPx, px);
  notify();
}

function easeOutCubic(t) {
  return 1 - Math.pow(1 - t, 3);
}

function prefersReducedMotion() {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

function animateSidebarWidth(fromPx, toPx) {
  const root = document.documentElement;
  if (prefersReducedMotion() || Math.abs(fromPx - toPx) < 1) {
    root.classList.remove('sidebar-resizing');
    root.style.removeProperty('--sidebar-w');
    invoke('set_sidebar_width', { px: toPx });
    return;
  }

  const start = performance.now();
  invoke('set_sidebar_width', { px: toPx });

  const step = (now) => {
    const t = Math.min(1, (now - start) / SIDEBAR_ANIM_MS);
    const px = fromPx + (toPx - fromPx) * easeOutCubic(t);
    root.style.setProperty('--sidebar-w', `${px.toFixed(2)}px`);
    if (t < 1) {
      sidebarAnim = requestAnimationFrame(step);
    } else {
      root.classList.remove('sidebar-resizing');
      root.style.removeProperty('--sidebar-w');
    }
  };

  sidebarAnim = requestAnimationFrame(step);
}

export async function newTab() {
  await invoke('create_tab');
  document.getElementById('url').focus();
}

/* ---- keyboard nav inside the list ----------------------------------------- */

function onListKeydown(e) {
  const row = e.target.closest('.tab');
  if (!row) return;
  const rows = liveRows();
  const i = rows.indexOf(row);
  const id = Number(row.dataset.id);
  switch (e.key) {
    case 'ArrowDown': rows[Math.min(i + 1, rows.length - 1)]?.focus(); break;
    case 'ArrowUp': rows[Math.max(i - 1, 0)]?.focus(); break;
    case 'Home': rows[0]?.focus(); break;
    case 'End': rows[rows.length - 1]?.focus(); break;
    case 'Enter': case ' ': invoke('activate_tab', { id }); break;
    case 'Delete': case 'Backspace': invoke('close_tab', { id }); break;
    default: return;
  }
  e.preventDefault();
}

/* ---- init ----------------------------------------------------------------- */

export function init() {
  document.getElementById('new-tab').addEventListener('click', newTab);
  toggleBtn.addEventListener('click', toggleCollapsed);
  toggleBtn.setAttribute('aria-expanded', String(!state.collapsed));
  if (state.collapsed) {
    toggleBtn.setAttribute('aria-label', 'Expand sidebar');
    toggleBtn.title = 'Expand sidebar';
  }
  listEl.addEventListener('scroll', updateFade, { passive: true });
  listEl.addEventListener('keydown', onListKeydown);
  new ResizeObserver(updateFade).observe(listEl);

  initReorder(listEl, (id, toIndex) => {
    const from = state.tabs.findIndex((t) => t.id === id);
    if (from < 0) return;
    const [tab] = state.tabs.splice(from, 1);
    state.tabs.splice(toIndex, 0, tab);
    notify();
    invoke('move_tab', { id, toIndex });
  });
}

export function sidebarPx() {
  return state.collapsed ? COLLAPSED_PX : EXPANDED_PX;
}
