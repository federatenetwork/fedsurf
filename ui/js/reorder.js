// Drag-to-reorder for the tab list. Manual pointer events (HTML5 DnD's ghost
// image and cursor are unfixable): 4px threshold, dragged row translates,
// siblings FLIP-shift by one row height.

const ROW_H = 38; // 36px row + 2px gap

let suppressUntil = 0;

/** Sidebar click handlers call this to ignore the click that ends a drag. */
export function clickWasDrag() {
  return performance.now() < suppressUntil;
}

export function initReorder(listEl, onMove) {
  let drag = null;

  listEl.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    const row = e.target.closest('.tab');
    if (!row || e.target.closest('.tab-close') || row.classList.contains('exiting')) return;
    drag = { row, id: Number(row.dataset.id), startY: e.clientY, active: false, lastTarget: null };
  });

  window.addEventListener('pointermove', (e) => {
    if (!drag) return;
    const dy = e.clientY - drag.startY;
    if (!drag.active) {
      if (Math.abs(dy) < 4) return;
      drag.active = true;
      drag.siblings = [...listEl.querySelectorAll('.tab:not(.exiting)')].filter((n) => n !== drag.row);
      drag.startIndex = [...listEl.querySelectorAll('.tab:not(.exiting)')].indexOf(drag.row);
      drag.row.classList.add('dragging');
      document.documentElement.classList.add('reordering');
    }
    const maxDy = (drag.siblings.length - drag.startIndex) * ROW_H;
    const minDy = -drag.startIndex * ROW_H;
    const clamped = Math.max(minDy, Math.min(dy, maxDy));
    drag.row.style.transform = `translateY(${clamped}px)`;

    const target = Math.max(0, Math.min(drag.startIndex + Math.round(clamped / ROW_H), drag.siblings.length));
    if (target !== drag.lastTarget) {
      drag.lastTarget = target;
      drag.siblings.forEach((sib, i) => {
        // i = sibling's index in the list with the dragged row removed
        let shift = 0;
        if (i >= target && i < drag.startIndex) shift = ROW_H;
        else if (i < target && i >= drag.startIndex) shift = -ROW_H;
        sib.classList.add('shifting');
        sib.style.transform = shift ? `translateY(${shift}px)` : '';
      });
    }
  });

  const finish = () => {
    if (!drag) return;
    const d = drag;
    drag = null;
    if (!d.active) return;
    suppressUntil = performance.now() + 300;
    d.row.classList.remove('dragging');
    d.row.style.transform = '';
    document.documentElement.classList.remove('reordering');
    for (const sib of d.siblings) {
      sib.classList.remove('shifting');
      sib.style.transform = '';
    }
    if (d.lastTarget !== null && d.lastTarget !== d.startIndex) onMove(d.id, d.lastTarget);
  };
  window.addEventListener('pointerup', finish);
  window.addEventListener('pointercancel', finish);
}
