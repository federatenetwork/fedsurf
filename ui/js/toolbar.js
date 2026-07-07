// Address bar, scheme badge, nav buttons, loading line.

import { state, activeTab } from './store.js';

const { invoke } = window.__TAURI__.core;

const urlInput = document.getElementById('url');
const schemeEl = document.getElementById('scheme');
const schemeIcon = document.getElementById('scheme-icon');
const schemeLabel = document.getElementById('scheme-label');
const progress = document.getElementById('progress');
const backBtn = document.getElementById('back');
const forwardBtn = document.getElementById('forward');
const reloadBtn = document.getElementById('reload');
const homeBtn = document.getElementById('home');

// Tabler Icons (outline): shield-check / lock / alert-triangle
const ICONS = {
  fed: '<path d="M11.46 20.846a12 12 0 0 1 -7.96 -14.846a12 12 0 0 0 8.5 -3a12 12 0 0 0 8.5 3a12 12 0 0 1 -.09 7.06"/><path d="M15 19l2 2l4 -4"/>',
  https: '<path d="M5 13a2 2 0 0 1 2 -2h10a2 2 0 0 1 2 2v6a2 2 0 0 1 -2 2h-10a2 2 0 0 1 -2 -2v-6"/><path d="M11 16a1 1 0 1 0 2 0a1 1 0 0 0 -2 0"/><path d="M8 11v-4a4 4 0 1 1 8 0v4"/>',
  http: '<path d="M12 9v4"/><path d="M10.363 3.591l-8.106 13.534a1.914 1.914 0 0 0 1.636 2.871h16.214a1.914 1.914 0 0 0 1.636 -2.87l-8.106 -13.536a1.914 1.914 0 0 0 -3.274 0"/><path d="M12 16h.01"/>',
};
const LABELS = { fed: 'FED · VERIFIED', https: 'HTTPS', http: 'HTTP · NOT SECURE' };

let shownUrl = '';
let shownKind = 'none';

/* WebView2 surfaces fed:// as https://fed.localhost/…; show the canonical
   form so the address bar reads the same on every OS. */
function displayUrl(url) {
  for (const prefix of ['https://fed.localhost/', 'http://fed.localhost/']) {
    if (url.startsWith(prefix)) return 'fed://' + url.slice(prefix.length);
  }
  return url;
}

function kindOf(url) {
  if (url.startsWith('fed://')) return 'fed';
  if (url.startsWith('https://')) return 'https';
  if (url.startsWith('http://')) return 'http';
  return 'none';
}

function setUrl(url) {
  url = displayUrl(url);
  if (url === shownUrl) return;
  shownUrl = url;
  if (document.activeElement !== urlInput) urlInput.value = url;

  const kind = kindOf(url);
  if (kind === shownKind) return;
  const apply = () => {
    schemeEl.dataset.kind = kind;
    if (kind !== 'none') {
      schemeIcon.innerHTML = ICONS[kind];
      schemeLabel.textContent = LABELS[kind];
    }
    schemeEl.classList.remove('swapping');
  };
  // Crossfade the badge when the scheme actually changes.
  if (shownKind === 'none' || kind === 'none') apply();
  else {
    schemeEl.classList.add('swapping');
    setTimeout(apply, 140);
  }
  shownKind = kind;
}

export function focusAddress() {
  urlInput.focus();
  urlInput.select();
}

export function render() {
  const tab = activeTab();
  setUrl(tab ? tab.url : '');
  progress.dataset.loading = String(Boolean(tab && tab.loading));
  syncNavButton(backBtn, tab?.canBack);
  syncNavButton(forwardBtn, tab?.canForward);
}

export function init() {
  urlInput.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter') {
      const input = urlInput.value.trim();
      if (!input) return;
      try {
        await invoke('navigate', { input });
        urlInput.blur();
      } catch (err) {
        console.error('navigate failed:', err);
      }
    } else if (e.key === 'Escape') {
      urlInput.value = shownUrl;
      urlInput.blur();
    }
  });
  urlInput.addEventListener('focus', () => urlInput.select());

  backBtn.addEventListener('click', () => {
    pulse(backBtn, 'nudge-left');
    invoke('go_back');
  });
  forwardBtn.addEventListener('click', () => {
    pulse(forwardBtn, 'nudge-right');
    invoke('go_forward');
  });
  reloadBtn.addEventListener('click', () => {
    reloadBtn.classList.add('spinning');
    invoke('reload');
  });
  reloadBtn.addEventListener('animationend', () => reloadBtn.classList.remove('spinning'));
  homeBtn.addEventListener('click', () => {
    pulse(homeBtn, 'hop');
    invoke('go_home');
  });
}

function pulse(btn, cls) {
  if (btn.disabled) return;
  if (btn._pulseUntil && performance.now() < btn._pulseUntil) return;
  btn._pulseUntil = performance.now() + 130;
  btn.classList.remove(cls);
  void btn.offsetWidth;
  btn.classList.add(cls);
  window.setTimeout(() => btn.classList.remove(cls), 220);
}

function syncNavButton(btn, canNavigate) {
  const disabled = canNavigate === false;
  btn.disabled = disabled;
  btn.setAttribute('aria-disabled', String(disabled));
}
