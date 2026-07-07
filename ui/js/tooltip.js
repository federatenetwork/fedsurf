// One shared tooltip for the collapsed rail: title + url, 8px right of the
// row, 500ms intent delay, instant when moving between rows.

const el = document.getElementById('tooltip');
const titleEl = document.createElement('div');
const urlEl = document.createElement('div');
titleEl.className = 'tt-title';
urlEl.className = 'tt-url';
el.append(titleEl, urlEl);

let showTimer = 0;
let hideTimer = 0;
let visible = false;

function show(node, data) {
  titleEl.textContent = data.title;
  urlEl.textContent = data.url;
  urlEl.hidden = !data.url;
  const rect = node.getBoundingClientRect();
  el.style.left = `${rect.right + 8}px`;
  // Content is set, so offsetHeight is current even while transparent.
  const top = rect.top + rect.height / 2 - el.offsetHeight / 2;
  el.style.top = `${Math.max(4, Math.min(top, window.innerHeight - el.offsetHeight - 4))}px`;
  el.classList.add('visible');
  visible = true;
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
  // Short grace so hopping to the next row keeps the tooltip instant.
  clearTimeout(hideTimer);
  hideTimer = setTimeout(() => { visible = false; }, 150);
}
