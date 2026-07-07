// One shared tooltip for the collapsed rail: title + url, 8px right of the
// row, 500ms intent delay, instant when moving between rows.

const el = document.getElementById('tooltip');
const { invoke } = window.__TAURI__.core;
const COLLAPSED_PX = 52;
const MAX_GUTTER_PX = 360;
const titleEl = document.createElement('div');
const urlEl = document.createElement('div');
titleEl.className = 'tt-title';
urlEl.className = 'tt-url';
el.append(titleEl, urlEl);

let showTimer = 0;
let hideTimer = 0;
let visible = false;
let gutterOpen = false;

function show(node, data) {
  titleEl.textContent = data.title;
  urlEl.textContent = data.url;
  urlEl.hidden = !data.url;
  const rect = node.getBoundingClientRect();
  el.style.left = `${rect.right + 8}px`;
  // Content is set, so offsetHeight is current even while transparent.
  const top = rect.top + rect.height / 2 - el.offsetHeight / 2;
  el.style.top = `${Math.max(4, Math.min(top, window.innerHeight - el.offsetHeight - 4))}px`;
  openGutter(rect);
  el.classList.add('visible');
  visible = true;
}

function openGutter(rect) {
  if (!document.documentElement.classList.contains('collapsed')) return;
  if (document.documentElement.classList.contains('sidebar-resizing')) return;
  const required = Math.ceil(rect.right + 8 + el.offsetWidth + 12);
  invoke('set_sidebar_width', { px: Math.max(COLLAPSED_PX, Math.min(required, MAX_GUTTER_PX)) })
    .catch(() => {});
  gutterOpen = true;
}

function closeGutter() {
  if (!gutterOpen) return;
  gutterOpen = false;
  if (document.documentElement.classList.contains('collapsed')) {
    invoke('set_sidebar_width', { px: COLLAPSED_PX }).catch(() => {});
  }
}

export function attach(node, getData) {
  node.addEventListener('mouseenter', () => {
    if (!document.documentElement.classList.contains('collapsed')) return;
    clearTimeout(showTimer);
    clearTimeout(hideTimer);
    showTimer = setTimeout(() => show(node, getData()), visible ? 0 : 500);
  });
  node.addEventListener('mouseleave', hide);
  node.addEventListener('pointerdown', hide);
}

export function hide() {
  clearTimeout(showTimer);
  el.classList.remove('visible');
  closeGutter();
  // Short grace so hopping to the next row keeps the tooltip instant.
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => { visible = false; }, 150);
}
