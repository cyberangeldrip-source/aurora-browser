'use strict';

(() => {
  const hint = document.createElement('div');
  hint.className = 'aurora-tooltip';
  hint.setAttribute('role', 'tooltip');
  hint.hidden = true;
  document.body.append(hint);
  let trigger;
  let timer;
  const autoLabels = new WeakSet();

  function migrate(node) {
    const title = node.getAttribute('title');
    if (title === null) return;
    node.removeAttribute('title');
    if (title) {
      node.dataset.tooltip = title;
      if (autoLabels.has(node) || (node.matches('button, a') && !node.hasAttribute('aria-label') && !node.textContent.trim())) {
        node.setAttribute('aria-label', title);
        autoLabels.add(node);
      }
    } else {
      delete node.dataset.tooltip;
    }
  }
  function migrateTree(node) {
    if (!(node instanceof HTMLElement)) return;
    migrate(node);
    for (const el of node.querySelectorAll('[title]')) migrate(el);
  }

  function hide() {
    clearTimeout(timer);
    trigger = undefined;
    hint.hidden = true;
  }

  migrateTree(document.body);
  new MutationObserver(records => {
    for (const record of records) {
      if (record.type === 'attributes') {
        if (record.target instanceof HTMLElement) migrate(record.target);
      } else {
        for (const node of record.addedNodes) migrateTree(node);
      }
    }
  }).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['title'] });

  document.addEventListener('pointerover', event => {
    const target = event.target.closest?.('[data-tooltip]');
    if (trigger === target) return;
    hide();
    if (!target || event.pointerType === 'touch') return;
    trigger = target;
    timer = setTimeout(() => {
      if (!target.isConnected || !target.dataset.tooltip) return hide();
      hint.textContent = target.dataset.tooltip;
      hint.hidden = false;
      const anchor = target.getBoundingClientRect();
      const box = hint.getBoundingClientRect();
      const x = Math.max(5, Math.min(innerWidth - box.width - 5, anchor.left + (anchor.width - box.width) / 2));
      const y = anchor.bottom + box.height + 8 <= innerHeight
        ? anchor.bottom + 5 : Math.max(5, anchor.top - box.height - 5);
      hint.style.left = x + 'px';
      hint.style.top = y + 'px';
    }, 520);
  });
  document.addEventListener('pointerout', event => {
    const next = event.relatedTarget;
    if (trigger && (!(next instanceof Node) || !trigger.contains(next))) hide();
  });
  for (const type of ['pointerdown', 'keydown', 'scroll']) document.addEventListener(type, hide, true);
  window.addEventListener('blur', hide);
  window.addEventListener('resize', hide);
})();
