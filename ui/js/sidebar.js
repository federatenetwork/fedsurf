// The vertical tab list: keyed DOM rendering (nodes are reused and reordered,
// never wiped), enter/exit animations, favicon/letter-tile/spinner slot,
// roving-tabindex keyboard nav, collapse toggle.

import { state, notify, hostOf } from './store.js';
import { initReorder, clickWasDrag } from './reorder.js';
import { attach as attachTooltip } from './tooltip.js';

const { invoke } = window.__TAURI__.core;

const listEl = document.getElementById('tab-list');
const toggleBtn = document.getElementById('toggle-sidebar');
const nodes = new Map(); // id -> row element

const EXPANDED_PX = 240;
const COLLAPSED_PX = 52;
const SIDEBAR_ANIM_MS = 200;
const TILE_COLORS = ['var(--teal)', 'var(--navy)', 'var(--olive)', 'var(--terracotta)', 'var(--amber)'];
const CLOSE_SVG =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6l-12 12"/><path d="M6 6l12 12"/></svg>';
const BRAND_SVG =
  '<svg viewBox="0 0 400 400" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M218.807 105.212C217.399 129.128 216.695 141.086 223.269 143.809C229.842 146.532 237.801 137.58 253.719 119.676L309.658 56.7584C316.267 49.3252 319.571 45.6085 323.884 45.482C328.197 45.3555 331.714 48.872 338.747 55.9051L344.102 61.2603C351.136 68.2945 354.653 71.8115 354.526 76.1252C354.399 80.4388 350.682 83.743 343.246 90.3515L280.328 146.272C262.418 162.189 253.464 170.148 256.186 176.722C258.909 183.296 270.869 182.593 294.788 181.186L378.826 176.245C388.756 175.661 393.721 175.369 396.86 178.33C400 181.29 400 186.264 400 196.211V203.788C400 213.736 400 218.709 396.86 221.67C393.72 224.63 388.755 224.338 378.825 223.754L294.785 218.807C270.867 217.399 258.908 216.695 256.184 223.269C253.461 229.843 262.415 237.802 280.322 253.72L343.247 309.656C350.682 316.266 354.4 319.57 354.526 323.884C354.652 328.198 351.135 331.715 344.099 338.748L338.739 344.107C331.705 351.139 328.188 354.655 323.875 354.528C319.562 354.401 316.258 350.684 309.65 343.25L253.721 280.327C237.803 262.418 229.843 253.463 223.27 256.186C216.696 258.909 217.399 270.869 218.807 294.789L223.754 378.825C224.338 388.755 224.63 393.72 221.67 396.86C218.709 400 213.736 400 203.788 400H196.21C186.263 400 181.29 400 178.33 396.86C175.369 393.721 175.661 388.756 176.245 378.826L181.186 294.788C182.593 270.869 183.296 258.909 176.722 256.186C170.148 253.464 162.189 262.418 146.272 280.328L90.3515 343.246C83.743 350.682 80.4388 354.399 76.1252 354.526C71.8116 354.653 68.2945 351.136 61.2603 344.102L55.9051 338.747C48.872 331.714 45.3555 328.197 45.482 323.884C45.6085 319.571 49.3252 316.267 56.7584 309.658L119.676 253.719C137.58 237.801 146.533 229.842 143.809 223.269C141.086 216.695 129.128 217.399 105.212 218.807L21.1752 223.754C11.2449 224.338 6.27963 224.63 3.1398 221.67C-2.64645e-05 218.709 0 213.736 0 203.788V196.211C0 186.264 -3.79086e-05 181.29 3.13957 178.33C6.27918 175.369 11.2441 175.661 21.174 176.245L105.206 181.186C129.124 182.593 141.083 183.296 143.806 176.722C146.528 170.148 137.574 162.19 119.666 146.272L56.7547 90.3535C49.3188 83.7442 45.6008 80.4395 45.4743 76.1255C45.3478 71.8116 48.8657 68.2947 55.9014 61.2609L61.2615 55.9023C68.2953 48.8705 71.8121 45.3546 76.1251 45.4816C80.4381 45.6086 83.7419 49.3254 90.3497 56.7589L146.273 119.671C162.19 137.577 170.149 146.53 176.722 143.807C183.296 141.085 182.593 129.126 181.187 105.209L176.245 21.174C175.661 11.2442 175.369 6.27922 178.33 3.13961C181.29 0 186.264 0 196.211 0H203.788C213.736 0 218.709 0 221.67 3.13983C224.63 6.27965 224.338 11.2448 223.754 21.1752L218.807 105.212Z" fill="currentColor"/></svg>';

let lastActiveId = null;
let sidebarAnim = 0;
let sidebarTimer = 0;

/* ---- helpers ------------------------------------------------------------ */

function tileFor(url) {
  const host = hostOf(url);
  if (!host) return { letter: '+', color: 'var(--accent)' };
  let hash = 0;
  for (let i = 0; i < host.length; i++) hash = (hash * 31 + host.charCodeAt(i)) >>> 0;
  return { letter: host[0], color: TILE_COLORS[hash % TILE_COLORS.length] };
}

function isNewTab(tab) {
  return !tab.url;
}

function titleFor(tab) {
  if (isNewTab(tab)) return { text: 'New Tab', faded: true };
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

function canCloseFromEvent(e) {
  return !state.collapsed || e.metaKey || e.ctrlKey;
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
  const brand = document.createElement('span');
  brand.className = 'brand-symbol';
  brand.innerHTML = BRAND_SVG;
  const spinner = document.createElement('span');
  spinner.className = 'spinner';
  spinner.setAttribute('aria-hidden', 'true');
  icon.append(img, tile, brand, spinner);

  const title = document.createElement('span');
  title.className = 'tab-title';

  const close = document.createElement('button');
  close.className = 'tab-close';
  close.tabIndex = -1;
  close.innerHTML = CLOSE_SVG; // static markup, no user content

  row.append(icon, title, close);
  row._els = { img, tile, title, close };

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
    if (e.button === 1 && canCloseFromEvent(e)) invoke('close_tab', { id: tab.id });
  });
  close.addEventListener('click', (e) => {
    if (!canCloseFromEvent(e)) return;
    invoke('close_tab', { id: tab.id });
  });
  attachTooltip(row, () => row._tooltip || { title: '', url: '' });

  return row;
}

function syncIconMode(row) {
  const tab = state.tabs.find((t) => t.id === Number(row.dataset.id));
  if (!tab) return;
  const { img } = row._els;
  if (isNewTab(tab)) {
    row.dataset.icon = 'brand';
    row.removeAttribute('aria-busy');
    return;
  }
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
  const titleEl = row._els.title;
  if (titleEl.textContent !== text) titleEl.textContent = text;
  titleEl.classList.toggle('faded', faded);

  const { letter, color } = tileFor(tab.url);
  const tile = row._els.tile;
  tile.textContent = letter;
  tile.style.background = color;

  const selected = tab.id === state.activeId;
  row.setAttribute('aria-selected', String(selected));
  row.tabIndex = selected ? 0 : -1;
  row.setAttribute('aria-label', text);
  row.removeAttribute('title');
  row._tooltip = { title: text, url: tab.url || '' };
  row._els.close.setAttribute('aria-label', `Close ${text}`);

  syncIconMode(row);
}

/* ---- render -------------------------------------------------------------- */

export function render() {
  let prev = null;
  const liveIds = new Set(state.tabs.map((t) => t.id));
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
    if (!liveIds.has(id)) {
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
  clearTimeout(sidebarTimer);
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

/* Native tab webviews are expensive to relayout. The chrome sidebar animates
   in CSS, while the active native webview moves once on the side that avoids
   it covering the animated rail: before expand, after collapse. */
let pendingWidthPx = null;
let widthRpc = Promise.resolve();

function pushSidebarWidth(px) {
  pendingWidthPx = px;
  widthRpc = widthRpc.then(() => {
    if (pendingWidthPx == null) return undefined;
    const send = pendingWidthPx;
    pendingWidthPx = null;
    return invoke('set_sidebar_width', { px: send }).catch(() => {});
  });
}

function animateSidebarWidth(fromPx, toPx) {
  const root = document.documentElement;
  const expanding = toPx > fromPx;
  if (expanding) pushSidebarWidth(toPx);

  sidebarAnim = requestAnimationFrame(() => {
    root.style.setProperty('--sidebar-w', `${toPx.toFixed(2)}px`);
    sidebarTimer = window.setTimeout(() => {
      root.style.removeProperty('--sidebar-w');
      if (!expanding) pushSidebarWidth(toPx);
    }, SIDEBAR_ANIM_MS + 20);
  });
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
    case 'Delete': case 'Backspace':
      if (!canCloseFromEvent(e)) return;
      invoke('close_tab', { id });
      break;
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

  // Track ⌘/Ctrl for the collapsed rail: the close button only replaces the
  // favicon while the modifier is held (html.mod-down, see sidebar.css).
  const setMod = (down) => document.documentElement.classList.toggle('mod-down', down);
  window.addEventListener('keydown', (e) => {
    if (e.key === 'Meta' || e.key === 'Control') setMod(true);
  });
  window.addEventListener('keyup', (e) => {
    if (e.key === 'Meta' || e.key === 'Control') setMod(e.metaKey || e.ctrlKey);
  });
  window.addEventListener('blur', () => setMod(false));
  // Keys can go down while another webview has focus — mouse events still
  // carry live modifier state, so hovering the rail stays accurate.
  listEl.addEventListener('mousemove', (e) => setMod(e.metaKey || e.ctrlKey), { passive: true });

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
