'use strict';

const { core, event } = window.__TAURI__ ?? {};
const $ = (id) => document.getElementById(id);
const root = document.documentElement;
const address = $('address');
const chrome = $('chrome');
const menu = $('menu');
const tabContext = $('tab-context');
const panel = $('panel-overlay');
const bookmarkMenu = $('bookmark-menu');
let bookmarkAnchor;
const downloads = $('downloads');
const suggest = $('suggest');
const suggestions = $('suggest-items');
const settingsPanel = panel.querySelector('.panel');
const backdrop = $('overlay-backdrop');
const confirmDialog = $('confirm');

const icons = {
  back: '<path d="M19 12H5.5m0 0 5.4-5.4M5.5 12l5.4 5.4"/>',
  forward: '<path d="M5 12h13.5m0 0-5.4-5.4m5.4 5.4-5.4 5.4"/>',
  reload: '<path d="M19 11a7 7 0 1 0-2.1 5.8M19 5v6h-6"/>',
  home: '<path d="m4 11 8-7 8 7v8a1 1 0 0 1-1 1h-5v-5h-4v5H5a1 1 0 0 1-1-1z"/>',
  close: '<path d="m6 6 12 12M18 6 6 18"/>',
  minus: '<path d="M5 12h14"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  square: '<rect x="5" y="5" width="14" height="14" rx="2"/>',
  restore: '<rect x="4" y="8" width="12" height="12" rx="2"/><path d="M8 8V6a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-2"/>',
  arrow: '<path d="M5 12h13m-5-5 5 5-5 5"/>',
  link: '<path d="m10 14 4-4m-6 6-1 1a3.5 3.5 0 0 1-5-5l3.5-3.5a3.5 3.5 0 0 1 5 0m3 0 1-1a3.5 3.5 0 0 1 5 5L16 16a3.5 3.5 0 0 1-5 0" transform="translate(1 -1)"/>',
  tabs: '<rect x="4" y="7" width="13" height="13" rx="2"/><path d="M8 7V5a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1h-2"/>',
  star: '<path d="m12 3 2.8 5.6 6.2.9-4.5 4.4 1 6.1-5.5-2.9-5.5 2.9 1-6.1L3 9.5l6.2-.9z"/>',
  download: '<path d="M12 4v10m-3.8-3.8L12 14l3.8-3.8M4.5 15.5V18A2.5 2.5 0 0 0 7 20.5h10a2.5 2.5 0 0 0 2.5-2.5v-2.5"/>',
  import: '<path d="M6 6h12a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-3m2-4h8m-3-3 3 3-3 3"/>',
  account: '<circle cx="12" cy="8" r="3.3"/><path d="M5 20v-1.2a7 7 0 0 1 14 0V20"/>',
  folder: '<path d="M3.5 7.5A2 2 0 0 1 5.5 5.5H9l2 2h7.5a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/>',
  pause: '<path d="M9 5v14M15 5v14"/>',
  play: '<path d="m8 5 12 7-12 7z"/>',
  history: '<circle cx="12" cy="12" r="8"/><path d="M12 7v5l3 2"/>',
  menu: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
  fullscreen: '<path d="M4 10V4h6m4 0h6v6m0 4v6h-6m-4 0H4v-6"/>',
  search: '<circle cx="10.5" cy="10.5" r="6"/><path d="m15 15 5 5"/>',
  lock: '<rect x="5" y="10" width="14" height="10" rx="2"/><path d="M8 10V8a4 4 0 0 1 8 0v2"/>',
  globe: '<circle cx="12" cy="12" r="8"/><ellipse cx="12" cy="12" rx="3.5" ry="8"/><path d="M4 12h16"/>',
  private: '<circle cx="7" cy="15" r="3.5"/><circle cx="17" cy="15" r="3.5"/><path d="M10.5 15h3M4 9h16M7 9l1.5-4h7L17 9"/>',
  alert: '<path d="m10.5 4-8 14a1.5 1.5 0 0 0 1.3 2h16.4a1.5 1.5 0 0 0 1.3-2l-8-14a1.7 1.7 0 0 0-3 0ZM12 9v4m0 3h.01"/>',
  settings: '<path d="M4 6h6m4 0h6M4 12h10m4 0h2M4 18h3m4 0h9"/><circle cx="12" cy="6" r="2"/><circle cx="16" cy="12" r="2"/><circle cx="9" cy="18" r="2"/>',
  plugin: '<path d="M8 5h8v5h4v4h-4v4a3 3 0 0 1-3 3h-2a3 3 0 0 1-3-3v-4H4v-4h4z"/>',
  info: '<circle cx="12" cy="12" r="8"/><path d="M12 11v5m0-8h.01"/>',
};
function icon(name) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('stroke-width', '1.75');
  svg.setAttribute('aria-hidden', 'true');
  svg.innerHTML = icons[name];
  return svg;
}
for (const node of document.querySelectorAll('[data-icon]')) node.prepend(icon(node.dataset.icon));

const isWindows = /Windows/.test(navigator.userAgent);
let state = { settings: null, tabs: [], bookmarks: [], zoom: 1 };
let settingsDraft;
let settingsQueue = Promise.resolve();
let latestSave = 0;
let currentPanel = '';
let editorId;
let selectedSuggestion = -1;
let suggestionRows = [];
let suggestionRequest = 0;
let suggestionTimer;
let toastTimer;
let currentOverlay = 0;
let layerQueued = false;
let layerTimer;
let glassIntent;
let contextTabId;
let syncPending = false;
let syncAgain = false;
let fileExportUrl;
let tabSignature = '';
let tabsRendered = false;
let editAddress = false;
let appliedSettings;

function toast(message) {
  const node = $('toast');
  node.textContent = typeof message === 'string' ? message : String(message?.message ?? message);
  node.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { node.hidden = true; }, 3200);
}
async function run(command, args) {
  try {
    const result = await core.invoke(command, args);
    return result === null || result === undefined ? true : result;
  } catch (error) {
    toast(error);
    return null;
  }
}
function internalPath(raw) {
  try {
    const url = new URL(raw);
    if (url.protocol === 'aurora:') return url.host === 'app' ? url.pathname.slice(1) : url.host;
    if (url.origin === 'http://aurora.localhost') return url.pathname.slice(1);
  } catch {}
  return null;
}
function displayUrl(raw) {
  const path = internalPath(raw);
  return path === null ? raw : !path || path === 'home' || path === 'private' ? '' : 'aurora://' + path;
}
function favicon(url, title, tab, bookmark) {
  const box = document.createElement('span');
  box.className = 'favicon';
  box.setAttribute('aria-hidden', 'true');
  if (internalPath(url) !== null) {
    const img = new Image(16, 16);
    img.alt = '';
    img.src = 'aurora-mark.svg';
    box.append(img);
  } else if (tab || bookmark) {
    const img = new Image(16, 16);
    img.alt = '';
    const origin = isWindows ? 'http://aurora.localhost' : 'aurora://app';
    img.addEventListener('error', () => box.replaceChildren(icon('globe')), { once: true });
    img.src = tab ? origin + '/res/tab-icon/' + tab.id + '?v=' + (tab.faviconRevision ?? 0) + '&theme=' + root.dataset.theme
      : origin + '/res/favorite/' + bookmark.id + '?theme=' + root.dataset.theme;
    box.append(img);
  } else {
    const key = (() => { try { return new URL(url).host; } catch { return url; } })();
    let hash = 5381;
    for (const char of key) hash = ((hash * 33) + char.codePointAt(0)) >>> 0;
    box.style.setProperty('--hue', hash % 360);
    box.textContent = (Array.from(title.trim())[0] || '•').toUpperCase();
  }
  return box;
}
function button(label, glyph, className, click) {
  const item = document.createElement('button');
  item.type = 'button';
  item.title = label;
  item.setAttribute('aria-label', label);
  item.className = className;
  item.append(icon(glyph));
  item.addEventListener('click', click);
  return item;
}
function toText(tag, className, value) {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = value;
  return node;
}

function showState(next) {
  const tabChanged = state.activeTabId !== next.activeTabId;
  const bookmarksChanged = state.settings?.theme !== next.settings?.theme
    || JSON.stringify(state.bookmarks) !== JSON.stringify(next.bookmarks);
  const pageChanged = tabChanged || state.currentUrl !== next.currentUrl;
  state = next;
  if (pageChanged) liquidGlass.setPage(glassPage().key);
  if (tabChanged || !editAddress) address.value = displayUrl(next.currentUrl);
  const site = $('site-icon');
  const internal = internalPath(next.currentUrl) !== null;
  const secure = next.currentUrl.startsWith('https:') && !next.failed;
  const siteSymbol = next.failed ? 'alert' : next.private ? 'private' : internal ? 'search' : secure ? 'lock' : 'globe';
  if (site.dataset.icon !== siteSymbol) {
    site.dataset.icon = siteSymbol;
    site.replaceChildren(icon(siteSymbol));
  }
  site.classList.toggle('failed', next.failed);
  site.title = next.failed ? 'Ошибка загрузки' : next.private ? 'Приватная вкладка' : internal ? 'Страница Aurora' : secure ? 'HTTPS' : 'Адрес сайта';

  $('progress').hidden = !next.loading;
  const reload = $('reload');
  const reloadAction = next.loading ? 'stop' : 'reload';
  if (reload.dataset.action !== reloadAction) {
    reload.dataset.action = reloadAction;
    reload.title = next.loading ? 'Остановить загрузку' : 'Обновить (Ctrl+R)';
    reload.replaceChildren(icon(next.loading ? 'close' : 'reload'));
  }
  $('back').disabled = isWindows && !next.canBack;
  $('forward').disabled = isWindows && !next.canForward;
  $('bookmark').disabled = !next.canBookmark;
  $('bookmark').setAttribute('aria-pressed', String(next.bookmarked));
  $('bookmark').title = next.bookmarked ? 'Убрать из закладок (Ctrl+D)' : 'Добавить в закладки (Ctrl+D)';
  $('zoom-label').textContent = Math.round(next.zoom * 100) + '%';
  for (const item of document.querySelectorAll('[data-command="reopen"]')) item.disabled = !next.canReopen;

  if (!settingsDraft) applySettings(next.settings);
  const nextSignature = JSON.stringify([next.activeTabId, root.dataset.theme, next.tabs]);
  if (tabSignature !== nextSignature) {
    tabSignature = nextSignature;
    renderTabs();
    if (currentPanel === 'tabs') renderTabList();
  }
  if (bookmarksChanged && currentPanel === 'bookmarks') renderBookmarks();
  if (bookmarksChanged) renderBookmarkStrip();
  if (contextTabId && !next.tabs.some(tab => tab.id === contextTabId)) dismissLayer();
  if (pageChanged && currentOverlay) setLayer();
  if (pageChanged && currentPanel === 'transfer') refreshPasswordAccounts();
  if (tabChanged && !downloads.hidden) {
    $('download-rows').replaceChildren();
    refreshDownloads();
  }
}

function scheduleSync() {
  if (syncPending) {
    syncAgain = true;
    return;
  }
  syncPending = true;
  requestAnimationFrame(async () => {
    const value = await run('ui_state');
    if (value) showState(value);
    syncPending = false;
    if (syncAgain) {
      syncAgain = false;
      scheduleSync();
    }
  });
}
async function resync() {
  const value = await run('ui_state');
  if (value) showState(value);
}

function renderTabs() {
  const list = $('tabs');
  tabStrip.style.setProperty('--tab-count', state.tabs.length);
  const incoming = [];
  const known = new Map(Array.from(list.children, (node) => [Number(node.dataset.id), node]));
  state.tabs.forEach((tab, index) => {
    let row = known.get(tab.id);
    const active = tab.id === state.activeTabId;
    if (!row) {
      row = document.createElement('div');
      row.className = 'tab';
      row.dataset.id = tab.id;
      const select = document.createElement('button');
      select.className = 'tab-select';
      select.type = 'button';
      select.setAttribute('role', 'tab');
      select.setAttribute('aria-keyshortcuts', 'Alt+Shift+ArrowLeft Alt+Shift+ArrowRight');
      select.addEventListener('click', () => selectTab(tab.id));
      row.append(select, button('Закрыть вкладку', 'close', 'tab-close', () => closeTab(tab.id)));
      incoming.push(row);
    }
    known.delete(tab.id);
    row.classList.toggle('active', active);
    row.classList.toggle('private', Boolean(tab.private));
    const select = row.firstElementChild;
    const title = tab.group ? (tab.group.name || 'Группа') + ' · ' + tab.title : tab.title;
    select.title = title;
    select.setAttribute('aria-label', title);
    row.classList.toggle('grouped-tab', Boolean(tab.group));
    if (tab.group) row.style.setProperty('--group-hue', [220,208,358,42,146,330,271,27,181][tab.group.color] ?? 220);
    else row.style.removeProperty('--group-hue');
    select.setAttribute('aria-selected', String(active));
    const site = internalPath(tab.url) !== null ? 'internal' : 'web';
    const glyphKey = [site, tab.faviconRevision ?? 0, root.dataset.theme, Boolean(tab.private)].join(':');
    if (row.dataset.glyph !== glyphKey) {
      row.dataset.glyph = glyphKey;
      const glyph = tab.private && site === 'internal' ? icon('private') : favicon(tab.url, tab.title, tab);
      select.replaceChildren(glyph, toText('span', 'tab-title', tab.title));
    } else if (select.lastElementChild.textContent !== tab.title) {
      select.lastElementChild.textContent = tab.title;
    }
    if (list.children[index] !== row) list.insertBefore(row, list.children[index] ?? null);
  });
  for (const node of known.values()) node.remove();
  if (!tabDrag?.moving && !finishTabSettle) revealTab(list.querySelector('.tab.active'));
  if (tabsRendered && liquidGlass.animated() && !tabDrag?.moving) {
    for (const row of incoming) {
      row.animate([{ opacity: 0, translate: '0 4px', scale: .96 }, { opacity: 1, translate: 'none', scale: 1 }], {
        duration: 185, easing: 'cubic-bezier(.18,.82,.24,1)',
      });
    }
  }
  tabsRendered = true;
}

const tabStrip = $('tabs-scroll');
const tabList = $('tabs');
let tabDrag;
let suppressTabClickUntil = 0;
let finishTabSettle;

function openTabContext(row, pointerX) {
  const id = Number(row.dataset.id);
  const tab = state.tabs.find(item => item.id === id);
  if (!tab) return;
  closeLayer();
  contextTabId = id;
  const anchor = pointerX ?? row.getBoundingClientRect().left + 12;
  const menuWidth = Math.min(242, window.innerWidth - 16);
  tabContext.style.left = Math.max(8, Math.min(anchor, window.innerWidth - menuWidth - 8)) + 'px';
  tabContext.querySelector('[data-tab-action="select"]').hidden = id === state.activeTabId;
  tabContext.querySelector('[data-tab-action="copy"]').hidden = !displayUrl(tab.url);
  tabContext.hidden = false;
  setLayer();
  tabContext.querySelector('button:not([hidden])').focus({ preventScroll: true });
}

tabList.addEventListener('contextmenu', e => {
  const row = e.target.closest('.tab');
  if (!row) return;
  e.preventDefault();
  finishTabSettle?.();
  openTabContext(row, e.clientX || undefined);
});

tabContext.addEventListener('keydown', e => {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(e.key)) return;
  e.preventDefault();
  const items = [...tabContext.querySelectorAll('button:not([hidden])')];
  const index = items.indexOf(document.activeElement);
  const next = e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 :
    (index + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
  items[next]?.focus({ preventScroll: true });
});
tabContext.addEventListener('click', async e => {
  const button = e.target.closest('[data-tab-action]');
  const tab = state.tabs.find(item => item.id === contextTabId);
  if (!button || button.disabled || !tab) return;
  const action = button.dataset.tabAction;
  if (action === 'copy') {
    try {
      await navigator.clipboard.writeText(displayUrl(tab.url));
      toast('Адрес скопирован');
    } catch {
      toast('Не удалось скопировать адрес');
    }
    if (tab.id === contextTabId) dismissLayer();
    return;
  }
  closeLayer();
  if (action === 'select') selectTab(tab.id);
  else if (action === 'close') closeTab(tab.id);
  else if (action === 'duplicate') run('add_tab', { url: tab.url, private: Boolean(tab.private) });
});

function revealTab(tab) {
  if (!tab || tabStrip.clientWidth === 0) return;
  const strip = tabStrip.getBoundingClientRect();
  const box = tab.getBoundingClientRect();
  if (box.left < strip.left) tabStrip.scrollLeft += box.left - strip.left - 2;
  else if (box.right > strip.right) tabStrip.scrollLeft += box.right - strip.right + 2;
}

tabStrip.addEventListener('wheel', (e) => {
  if (tabStrip.scrollWidth <= tabStrip.clientWidth || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
  e.preventDefault();
  tabStrip.scrollLeft += e.deltaY;
}, { passive: false });
tabList.addEventListener('auxclick', (e) => {
  if (e.button === 1) {
    const row = e.target.closest('.tab');
    if (row) closeTab(Number(row.dataset.id));
  }
});
tabList.addEventListener('keydown', async (e) => {
  if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
    const row = e.target.closest('.tab');
    if (row) {
      e.preventDefault();
      openTabContext(row);
    }
    return;
  }
  if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
  if (e.altKey && !e.shiftKey) return;
  e.preventDefault();
  const direction = e.key === 'ArrowRight' ? 1 : -1;
  if (!e.altKey || !e.shiftKey) {
    cycle(direction);
    return;
  }
  e.stopPropagation();
  const id = Number(e.target.closest('.tab')?.dataset.id);
  const from = state.tabs.findIndex(tab => tab.id === id);
  if (from < 0 || !state.tabs[from + direction]) return;
  const before = state.tabs[from + (direction < 0 ? -1 : 2)]?.id ?? null;
  if (await run('move_tab', { id, before })) {
    await resync();
    revealTab(tabList.querySelector('[data-id="' + id + '"]'));
  }
});

tabList.addEventListener('dragstart', (e) => e.preventDefault());
tabList.addEventListener('pointerdown', (e) => {
  const row = e.target.closest('.tab-select')?.closest('.tab');
  if (e.button !== 0 || !e.isPrimary || !row || tabDrag) return;
  finishTabSettle?.();
  tabDrag = { row, id: Number(row.dataset.id), pointer: e.pointerId, originX: e.clientX, originY: e.clientY };
});
function liftTab(drag) {
  const bounds = drag.row.getBoundingClientRect();
  drag.offset = drag.originX - bounds.left;
  drag.top = bounds.top;
  drag.ghost = drag.row.cloneNode(true);
  drag.ghost.classList.add('tab-ghost');
  drag.ghost.setAttribute('aria-hidden', 'true');
  drag.ghost.inert = true;
  drag.ghost.style.width = bounds.width + 'px';
  drag.ghost.style.height = bounds.height + 'px';
  drag.ghost.style.transform = tabTranslation(bounds.left, bounds.top);
  drag.returnFocus = drag.row.contains(document.activeElement);
  document.body.append(drag.ghost);
  drag.row.classList.add('moving');
  document.body.classList.add('tab-dragging');
  tabList.classList.add('is-reordering');
  if (liquidGlass.animated()) {
    drag.ghost.style.translate = '0 -2px';
    drag.lift = drag.ghost.animate([{ translate: '0 0px' }, { translate: '0 -2px' }], {
      duration: 140, easing: 'cubic-bezier(.18,.82,.24,1)',
    });
  }
}
function tabTranslation(x, y) {
  const density = window.devicePixelRatio || 1;
  return 'translate3d(' + Math.round(x * density) / density + 'px,' + Math.round(y * density) / density + 'px,0)';
}
function markTabDrop() {
  if (!tabDrag?.moving) return;
  const drag = tabDrag;
  drag.frame = 0;
  const strip = tabStrip.getBoundingClientRect();
  const offset = drag.x < strip.left + 28 ? -14 : drag.x > strip.right - 28 ? 14 : 0;
  const previous = tabStrip.scrollLeft;
  if (offset) tabStrip.scrollLeft += offset;
  const width = drag.row.offsetWidth;
  drag.left = drag.x - drag.offset;
  drag.y = drag.top + Math.max(-5, Math.min(14, (drag.yPointer - drag.originY) * .30));
  drag.ghost.style.transform = tabTranslation(drag.left, drag.y);
  const center = drag.left + width / 2 - strip.left + tabStrip.scrollLeft;
  const rows = Array.from(tabList.children);
  const from = rows.indexOf(drag.row);
  const remaining = rows.filter(row => row !== drag.row);
  const before = remaining.find(row => center < row.offsetLeft + row.offsetWidth / 2);
  const to = before ? remaining.indexOf(before) : remaining.length;
  drag.before = before ? Number(before.dataset.id) : null;
  if (drag.to !== to || drag.width !== width || drag.from !== from) {
    drag.to = to;
    drag.from = from;
    drag.width = width;
    const displacement = width + parseFloat(getComputedStyle(tabList).columnGap);
    rows.forEach((row, index) => {
      const shift = index > from && index <= to ? -displacement
        : index < from && index >= to ? displacement : 0;
      row.style.setProperty('--tab-offset', shift + 'px');
    });
  }
  if (tabStrip.scrollLeft !== previous) drag.frame = requestAnimationFrame(markTabDrop);
}
window.addEventListener('pointermove', (e) => {
  if (!tabDrag || e.pointerId !== tabDrag.pointer) return;
  const drag = tabDrag;
  if (!drag.moving) {
    if (Math.abs(e.clientX - drag.originX) < 5 || Math.abs(e.clientY - drag.originY) > 24) return;
    drag.moving = true;
    tabList.setPointerCapture(e.pointerId);
    liftTab(drag);
  }
  drag.x = e.clientX;
  drag.yPointer = e.clientY;
  if (!drag.frame) drag.frame = requestAnimationFrame(markTabDrop);
});
async function finishTabDrag(e, commit) {
  if (!tabDrag || e.pointerId !== tabDrag.pointer) return;
  const drag = tabDrag;
  if (drag.moving && commit) {
    cancelAnimationFrame(drag.frame);
    drag.x = e.clientX;
    drag.yPointer = e.clientY;
    markTabDrop();
  }
  tabDrag = null;
  if (!drag.moving) return;
  cancelAnimationFrame(drag.frame);
  suppressTabClickUntil = performance.now() + 90;
  document.body.classList.remove('tab-dragging');
  const tidy = () => {
    if (finishTabSettle !== tidy) return;
    tabList.classList.remove('is-reordering');
    for (const row of tabList.children) row.style.removeProperty('--tab-offset');
    drag.row.classList.remove('moving');
    drag.ghost.remove();
    finishTabSettle = null;
    if (drag.returnFocus && document.activeElement === document.body && drag.row.isConnected) {
      drag.row.firstElementChild.focus({ preventScroll: true });
    }
  };
  finishTabSettle = tidy;
  const strip = tabStrip.getBoundingClientRect();
  const valid = commit && e.clientY >= strip.top - 22 && e.clientY <= strip.bottom + 22;
  if (valid && drag.from !== drag.to && drag.row.isConnected) {
    if (await run('move_tab', { id: drag.id, before: drag.before ?? null })) await resync();
  }
  if (finishTabSettle !== tidy) return;
  tabList.classList.remove('is-reordering');
  for (const row of tabList.children) row.style.removeProperty('--tab-offset');
  if (!drag.row.isConnected || !liquidGlass.animated()) return tidy();
  const destination = drag.row.getBoundingClientRect();
  const lift = getComputedStyle(drag.ghost).translate;
  drag.lift?.cancel();
  const snap = drag.ghost.animate([
    { transform: drag.ghost.style.transform, translate: lift },
    { transform: tabTranslation(destination.left, destination.top), translate: '0 0px' },
  ], { duration: 205, easing: 'cubic-bezier(.17,.86,.27,1)', fill: 'forwards' });
  snap.finished.then(tidy, tidy);
  if (!valid && drag.row.classList.contains('active')) {
    snap.finished.then(() => revealTab(drag.row), () => {});
  }
}
window.addEventListener('pointerup', (e) => finishTabDrag(e, true));
window.addEventListener('pointercancel', (e) => finishTabDrag(e, false));
tabList.addEventListener('lostpointercapture', (e) => finishTabDrag(e, false));
tabList.addEventListener('click', (e) => {
  if (e.detail && performance.now() < suppressTabClickUntil) {
    e.preventDefault();
    e.stopImmediatePropagation();
  }
}, { capture: true });

async function newTab(privateTab = false) {
  closeLayer();
  if (await run('add_tab', { private: privateTab })) {
    await resync();
    focusAddress();
  }
}
function selectTab(id) {
  closeLayer();
  editAddress = false;
  return run('activate_tab', { id });
}
function closeTab(id) {
  return run('close_tab', { id });
}
function cycle(direction) {
  const tabs = state.tabs;
  if (!tabs.length) return;
  const at = tabs.findIndex((tab) => tab.id === state.activeTabId);
  selectTab(tabs[(at + direction + tabs.length) % tabs.length].id);
}

function glassPage() {
  const id = state.activeTabId;
  return {
    key: id ? id + ':' + window.innerWidth + ':' + state.currentUrl : null,
    read: id && isWindows ? () => core.invoke('glass_frame', { id }) : null,
  };
}
function prepareGlass() {
  if (state.loading) return;
  const { key, read } = glassPage();
  liquidGlass.prepare(key, read);
}
for (const trigger of [chrome, $('menu-button'), $('downloads-button'), address]) {
  trigger.addEventListener('pointerenter', () => {
    clearTimeout(glassIntent);
    glassIntent = setTimeout(prepareGlass, 55);
  });
  trigger.addEventListener('pointerleave', () => clearTimeout(glassIntent));
}

function setLayer() {
  if (layerQueued) return;
  layerQueued = true;
  queueMicrotask(() => {
    layerQueued = false;
    const surface = !panel.hidden ? settingsPanel : [bookmarkMenu, tabContext, downloads, menu, suggest].find(item => !item.hidden);
    backdrop.hidden = !surface || !panel.hidden;
    chrome.inert = !panel.hidden;
    $('menu-button').setAttribute('aria-expanded', String(!menu.hidden));
    $('downloads-button').setAttribute('aria-expanded', String(!downloads.hidden));
    clearTimeout(layerTimer);
    if (surface && currentOverlay === 0) {
      currentOverlay = 10000;
      run('set_ui_overlay', { pixels: currentOverlay });
    } else if (!surface && currentOverlay) {
      // Keep the chrome at one size until the exit finishes, including rapid close/reopen clicks.
      layerTimer = setTimeout(() => {
        currentOverlay = 0;
        run('set_ui_overlay', { pixels: 0 });
      }, liquidGlass.animated() ? 155 : 0);
    }
    const { key, read } = glassPage();
    liquidGlass.show(surface ?? null, key, read);
  });
}
function hideSuggestions() {
  clearTimeout(suggestionTimer);
  suggestionRequest++;
  selectedSuggestion = -1;
  suggestionRows = [];
  suggest.hidden = true;
  address.removeAttribute('aria-activedescendant');
  setLayer();
}
function closeLayer() {
  $('vault-secret').value = '';
  bookmarkMenu.hidden = true;
  contextTabId = undefined;
  tabContext.hidden = true;
  menu.hidden = true;
  downloads.hidden = true;
  clearTimeout(downloadTimer);
  panel.hidden = true;
  currentPanel = '';
  hideSuggestions();
}
function dismissLayer() {
  const folderButton = bookmarkMenu.hidden ? null : bookmarkAnchor;
  const target = contextTabId ? tabList.querySelector('[data-id="' + contextTabId + '"] .tab-select')
    : !downloads.hidden ? $('downloads-button') : !panel.hidden || !menu.hidden ? $('menu-button') : address;
  closeLayer();
  run('focus_chrome').then(() => {
    if (bookmarkMenu.hidden && tabContext.hidden && menu.hidden && panel.hidden && downloads.hidden && suggest.hidden) (folderButton ?? target)?.focus({ preventScroll: true });
  });
}

const downloadSizes = new Intl.NumberFormat('ru-RU', { maximumFractionDigits: 1 });
const downloadLabels = {
  running: 'Загружается',
  paused: 'На паузе',
  complete: 'Загружен',
  interrupted: 'Прервано',
  canceled: 'Отменено'
};
let downloadTimer;
let downloadRequest = 0;

function formatBytes(bytes) {
  const unit = Math.min(3, Math.floor(Math.log(Math.max(1, bytes)) / Math.log(1024)));
  return downloadSizes.format(bytes / 1024 ** unit) + ' ' + ['Б', 'КБ', 'МБ', 'ГБ'][unit];
}

function renderDownloadRows(items) {
  const list = $('download-rows');
  const known = new Map(Array.from(list.children, row => [Number(row.dataset.id), row]));
  $('downloads-empty').hidden = items.length > 0;
  items.forEach((item, index) => {
    let row = known.get(item.id);
    if (!row) {
      row = document.createElement('article');
      row.className = 'download-item';
      row.dataset.id = item.id;
      const text = document.createElement('div');
      text.className = 'item-text';
      const meter = document.createElement('progress');
      meter.setAttribute('aria-label', 'Ход загрузки');
      meter.className = 'download-meter';
      text.append(toText('strong', 'item-title', ''), toText('span', 'item-subtitle', ''), meter);
      row.append(icon('download'), text, toText('div', 'download-actions', ''));
    }
    known.delete(item.id);
    const title = row.querySelector('.item-title');
    title.textContent = title.title = item.name;
    const status = downloadLabels[item.status] ?? item.status;
    const size = item.total > 0 && item.status !== 'complete'
      ? formatBytes(item.received) + ' из ' + formatBytes(item.total)
      : item.received ? formatBytes(item.received) : '';
    row.querySelector('.item-subtitle').textContent = size ? status + ' · ' + size : status;
    const meter = row.querySelector('progress');
    meter.hidden = item.status !== 'running' && item.status !== 'paused';
    if (item.total > 0) {
      meter.max = item.total;
      meter.value = Math.min(item.received, item.total);
    } else {
      meter.removeAttribute('value');
    }
    const actions = [];
    if (item.status === 'running') actions.push(['pause', 'Пауза', 'pause']);
    else if (item.can_resume) actions.push(['resume', 'Возобновить', 'play']);
    if (item.status === 'running' || item.status === 'paused' || item.can_resume) {
      actions.push(['cancel', 'Отменить загрузку', 'close']);
    }
    if (item.status === 'complete') actions.push(['folder', 'Открыть папку файла', 'folder']);
    const signature = actions.map(([action]) => action).join(',');
    if (row.dataset.actions !== signature) {
      row.dataset.actions = signature;
      row.querySelector('.download-actions').replaceChildren(...actions.map(([action, label, glyph]) =>
        button(label, glyph, 'icon-button', async () => {
          if (await run('download_action', { id: item.id, action })) refreshDownloads();
        })));
    }
    if (list.children[index] !== row) list.insertBefore(row, list.children[index] ?? null);
  });
  for (const row of known.values()) row.remove();
}

async function refreshDownloads() {
  clearTimeout(downloadTimer);
  if (downloads.hidden) return;
  const request = ++downloadRequest;
  const items = await run('downloads_state');
  if (downloads.hidden || request !== downloadRequest || !Array.isArray(items)) return;
  renderDownloadRows(items);
  if (items.some(item => item.status === 'running')) {
    downloadTimer = setTimeout(refreshDownloads, 340);
  }
}

function toggleDownloads() {
  if (!downloads.hidden) return dismissLayer();
  closeLayer();
  downloads.hidden = false;
  setLayer();
  refreshDownloads();
  run('focus_chrome').then(() => {
    if (!downloads.hidden) $('downloads-close').focus({ preventScroll: true });
  });
}

async function focusAddress() {
  closeLayer();
  await run('focus_chrome');
  address.focus({ preventScroll: true });
  address.select();
}
function navigate(input) {
  editAddress = false;
  closeLayer();
  address.blur();
  return run('navigate', { input });
}

address.addEventListener('focus', () => {
  editAddress = true;
  address.select();
});
address.addEventListener('blur', () => {
  editAddress = false;
  address.value = displayUrl(state.currentUrl ?? '');
  hideSuggestions();
});
address.addEventListener('input', () => {
  const value = address.value.trim();
  if (!value) return hideSuggestions();
  clearTimeout(suggestionTimer);
  selectedSuggestion = -1;
  suggestionRows = [];
  address.removeAttribute('aria-activedescendant');
  const request = ++suggestionRequest;
  suggestionTimer = setTimeout(async () => {
    let matches;
    try { matches = await core.invoke('suggest', { q: value }); } catch { return; }
    if (request !== suggestionRequest || document.activeElement !== address) return;
    if (!matches.length) return hideSuggestions();
    suggestionRows = matches;
    const bounds = $('omnibox').getBoundingClientRect();
    suggest.style.width = bounds.width + 'px';
    suggest.style.left = bounds.left + 'px';
    suggestions.replaceChildren(...matches.map((match, index) => {
      const option = document.createElement('button');
      option.className = 'suggestion';
      option.type = 'button';
      option.tabIndex = -1;
      option.id = 'suggestion-' + index;
      option.setAttribute('role', 'option');
      const words = document.createElement('span');
      words.append(toText('strong', '', match.title), toText('small', '', match.url));
      option.append(icon(match.kind === 'bookmark' ? 'star' : 'history'), words);
      option.addEventListener('pointerdown', (event) => event.preventDefault());
      option.addEventListener('click', () => navigate(match.url));
      return option;
    }));
    suggest.hidden = false;
    setLayer();
  }, 105);
});
$('omnibox').addEventListener('submit', (e) => {
  e.preventDefault();
  const selected = suggestionRows[selectedSuggestion];
  navigate(selected ? selected.url : address.value.trim());
});
address.addEventListener('keydown', (e) => {
  if (!suggestionRows.length || !['ArrowUp', 'ArrowDown'].includes(e.key)) return;
  e.preventDefault();
  const length = suggestionRows.length;
  selectedSuggestion = e.key === 'ArrowDown'
    ? (selectedSuggestion + 1) % length
    : selectedSuggestion <= 0 ? length - 1 : selectedSuggestion - 1;
  Array.from(suggestions.children).forEach((row, index) => {
    row.setAttribute('aria-selected', String(index === selectedSuggestion));
  });
  const chosen = suggestions.children[selectedSuggestion];
  address.setAttribute('aria-activedescendant', chosen.id);
  chosen.scrollIntoView({ block: 'nearest' });
});

const settingsControls = [...document.querySelectorAll('[data-setting]')];
const palette = ['#7c5cff', '#438cfc', '#3eade0', '#2fac8c', '#eea342', '#ec6c9d', '#aa85d6'];
for (const color of palette) {
  const swatch = document.createElement('button');
  swatch.type = 'button';
  swatch.className = 'swatch';
  swatch.dataset.color = color;
  swatch.title = color;
  swatch.style.backgroundColor = color;
  swatch.addEventListener('click', () => changeSetting('accent', color, true));
  $('swatches').insertBefore(swatch, $('accent'));
}

function applySettings(settings) {
  if (!settings) return;
  const signature = JSON.stringify(settings);
  if (signature === appliedSettings) return;
  appliedSettings = signature;
  root.dataset.theme = settings.theme;
  root.classList.toggle('compact', settings.compact);
  root.classList.toggle('no-tabs', !settings.show_tab_row);
  root.classList.toggle('no-bookmark-bar', !settings.show_bookmark_bar);
  $('bookmark-bar').hidden = !settings.show_bookmark_bar;
  root.classList.toggle('no-motion', !settings.animations);
  root.style.setProperty('--accent', settings.accent);
  root.style.setProperty('--alpha', settings.glass_opacity);
  root.style.setProperty('--blur', settings.blur_strength + 'px');
  root.style.setProperty('--scale', settings.ui_scale);
  $('tabs-row').hidden = settings.show_tab_row === false;
  $('tab-list').hidden = settings.show_tab_row;
  $('compact-window-slot').hidden = settings.show_tab_row;
  const slot = settings.show_tab_row ? $('tabs-row') : $('compact-window-slot');
  if ($('window-controls').parentElement !== slot) slot.append($('window-controls'));
  for (const field of settingsControls) {
    const value = settings[field.dataset.setting];
    if (field.type === 'checkbox') field.checked = value;
    else if (document.activeElement !== field || field.type !== 'text') {
      if (field.id === 'retention' && !Array.from(field.options).some((option) => Number(option.value) === value)) {
        field.add(new Option(value + ' дн.', value));
      }
      field.value = field.id === 'homepage' && !displayUrl(value) ? 'aurora://home' : value;
    }
  }
  for (const swatch of document.querySelectorAll('.swatch')) {
    swatch.setAttribute('aria-pressed', String(swatch.dataset.color === settings.accent.toLowerCase()));
  }
  liquidGlass.repaint();
}
function changeSetting(name, value, persist) {
  if (!state.settings) return;
  settingsDraft = { ...(settingsDraft ?? state.settings), [name]: value };
  applySettings(settingsDraft);
  if (!persist) return;
  const revision = ++latestSave;
  const snapshot = settingsDraft;
  settingsQueue = settingsQueue.then(async () => {
    const saved = await run('save_settings', { settings: snapshot });
    if (saved) {
      const moved = saved.compact !== state.settings.compact || saved.show_tab_row !== state.settings.show_tab_row || saved.show_bookmark_bar !== state.settings.show_bookmark_bar;
      state.settings = saved;
      if (moved) liquidGlass.repaint();
    }
    if (revision === latestSave && settingsDraft === snapshot) {
      settingsDraft = null;
      applySettings(state.settings);
    }
  });
}
for (const field of settingsControls) {
  const value = () => (
    field.type === 'checkbox' ? field.checked :
      field.type === 'range' || Object.hasOwn(field.dataset, 'number') ? Number(field.value) : field.value
  );
  if (field.type === 'range' || field.type === 'color') {
    field.addEventListener('input', () => changeSetting(field.dataset.setting, value(), false));
  }
  field.addEventListener('change', () => changeSetting(field.dataset.setting, value(), true));
}

const migrationSelect = $('migration-source');
const migrationButton = $('migration-start');
const migrationFields = ['tabs', 'history', 'bookmarks', 'downloads'];
let migrationSources = [];
let migrationBusy = false;
let migrationScanning = false;
let accountRevision = 0;

function migrationStatus(id, text) {
  const status = $(id);
  status.textContent = text;
  status.hidden = !text;
}

function updateMigrationOptions() {
  const source = migrationSources.find(item => item.id === migrationSelect.value);
  for (const key of migrationFields) $('migration-' + key).disabled = !source?.[key] || migrationBusy;
  $('migration-retention').disabled = $('migration-history').disabled || !$('migration-history').checked;
  migrationButton.disabled = migrationBusy || !migrationFields.some(key => !$('migration-' + key).disabled && $('migration-' + key).checked);
}

async function refreshMigrationSources() {
  if (migrationScanning || migrationBusy) return;
  migrationScanning = true;
  $('migration-rescan').disabled = true;
  const selected = migrationSelect.value;
  migrationSources = await run('browser_sources') ?? [];
  const options = migrationSources.map(source => new Option(source.browser + ' · ' + source.profile, source.id));
  migrationSelect.replaceChildren(...(options.length ? options : [new Option('Браузеры не обнаружены', '')]));
  if (migrationSources.some(source => source.id === selected)) migrationSelect.value = selected;
  migrationSelect.disabled = !options.length;
  migrationScanning = false;
  $('migration-rescan').disabled = false;
  updateMigrationOptions();
}

function summaryText(report) {
  return ['Вкладки — ' + report.tabs, 'История — ' + report.history, 'Закладки — ' + report.bookmarks,
    'Журнал загрузок — ' + report.downloads,
    ...report.notices].join('\n');
}

migrationSelect.addEventListener('change', updateMigrationOptions);
$('migration-rescan').addEventListener('click', refreshMigrationSources);
for (const key of migrationFields) $('migration-' + key).addEventListener('change', updateMigrationOptions);
migrationButton.addEventListener('click', async () => {
  const fields = Object.fromEntries(migrationFields.map(key => {
    const field = $('migration-' + key);
    return [key, field.checked && !field.disabled];
  }));
  fields.keep_history = fields.history && $('migration-retention').checked;
  migrationBusy = migrationSelect.disabled = true;
  updateMigrationOptions();
  migrationStatus('migration-status', 'Перенос данных…');
  const result = await run('import_browser', { id: migrationSelect.value, fields });
  migrationBusy = migrationSelect.disabled = false;
  updateMigrationOptions();
  migrationStatus('migration-status', result ? summaryText(result) : 'Перенос завершился с ошибкой.');
});

async function refreshPasswordAccounts() {
  if (!isWindows) return;
  const revision = ++accountRevision;
  const id = state.activeTabId;
  const url = state.currentUrl;
  const accounts = await run('password_accounts') ?? [];
  if (revision !== accountRevision || id !== state.activeTabId || url !== state.currentUrl) return;
  $('migration-accounts-empty').hidden = accounts.length !== 0;
  $('migration-account-list').replaceChildren(...accounts.map(account => {
    const row = document.createElement('div');
    row.className = 'item';
    const label = toText('span', 'item-text', account.username || 'Без имени пользователя');
    const fill = button('Заполнить', 'lock', 'pill', async () => {
      if (id !== state.activeTabId || url !== state.currentUrl) return refreshPasswordAccounts();
      fill.disabled = true;
      const result = await run('fill_password', account);
      fill.disabled = false;
      if (result) {
        closeLayer();
        toast('Данные входа заполнены');
      }
    });
    fill.append(' Заполнить');
    row.append(label, fill);
    return row;
  }));
}

$('migration-csv').hidden = $('migration-vault-info').hidden = $('migration-accounts').hidden = !isWindows;
for (const [format, command, limit] of [['html', 'import_bookmark_html', 64], ['csv', 'import_password_csv', 8]]) {
  const trigger = $('migration-' + format);
  const field = $('migration-' + format + '-file');
  trigger.addEventListener('click', () => field.click());
  field.addEventListener('change', async () => {
    const file = field.files[0];
    field.value = '';
    if (!file) return;
    if (file.size > limit * 1024 * 1024) {
      migrationStatus('migration-file-status', 'Файл превышает допустимый размер.');
      return;
    }
    trigger.disabled = true;
    migrationStatus('migration-file-status', 'Чтение файла…');
    try {
      const result = await run(command, { payload: await file.text() });
      const status = format === 'csv' && result
        ? 'Пароли добавлены или обновлены — ' + result.imported + '. Пропущено — ' + result.skipped
        : result ? 'Добавлено закладок — ' + result.bookmarks : 'Импорт завершился с ошибкой.';
      migrationStatus('migration-file-status', status);
      if (format === 'csv' && result) refreshPasswordAccounts();
    } catch {
      migrationStatus('migration-file-status', 'Не удалось прочитать файл.');
    } finally {
      trigger.disabled = false;
    }
  });
}

const viewTitles = { settings: 'Настройки', tabs: 'Вкладки', bookmarks: 'Закладки', transfer: 'Перенос данных', profiles: 'Профили и пароли', plugins: 'Плагины', about: 'О браузере' };
function openPanel(view) {
  $('vault-secret').value = '';
  bookmarkMenu.hidden = true;
  contextTabId = undefined;
  tabContext.hidden = true;
  menu.hidden = true;
  downloads.hidden = true;
  clearTimeout(downloadTimer);
  currentPanel = view;
  panel.hidden = false;
  hideSuggestions();
  for (const key of Object.keys(viewTitles)) {
    $('view-' + key).hidden = view !== key;
    panel.querySelector('[data-view="' + key + '"]').setAttribute('aria-current', String(view === key));
  }
  $('panel-title').textContent = viewTitles[view];
  $('panel-body').scrollTop = 0;
  if (view === 'bookmarks') renderBookmarks();
  if (view === 'tabs') renderTabList();
  if (view === 'plugins') { loadPlugins(); loadBrowserExtensions(); }
  if (view === 'profiles') { loadProfiles(); loadVault(); }
  if (view === 'transfer') {
    refreshMigrationSources();
    refreshPasswordAccounts();
  }
  if (view === 'about') {
    $('version').textContent = state.meta?.version ?? '';
    $('data-dir').textContent = state.meta?.dataDir ?? '';
    $('engine-name').textContent = isWindows ? 'WebView2 (Chromium)' : 'WebKit';
  }
  setLayer();
  run('focus_chrome').then(() => {
    if (currentPanel === view) $('panel-close').focus({ preventScroll: true });
  });
}

function collectionItem(item, open, onClose, closeLabel, isTab = false) {
  const row = document.createElement('div');
  row.className = 'item';
  const link = toText('button', 'item-text', '');
  link.type = 'button';
  const subtitle = item.folder?.length ? item.folder.join(' / ') + ' · ' + item.url
    : item.group ? (item.group.name || 'Группа') + ' · ' + displayUrl(item.url) : displayUrl(item.url);
  link.append(toText('strong', 'item-title', item.title), toText('span', 'item-subtitle', subtitle));
  link.addEventListener('click', open);
  row.append(favicon(item.url, item.title, isTab ? item : null, isTab ? null : item), link, button(closeLabel, 'close', 'icon-button', onClose));
  return row;
}
function renderTabList() {
  $('tab-items').replaceChildren(...state.tabs.map((tab) => {
    const row = collectionItem(tab, () => selectTab(tab.id), () => closeTab(tab.id), 'Закрыть вкладку', true);
    row.classList.toggle('active', tab.id === state.activeTabId);
    return row;
  }));
}
function renderBookmarks() {
  const query = $('bookmark-filter').value.trim().toLocaleLowerCase('ru');
  const bookmarks = state.bookmarks.filter((b) => [b.title, b.url, ...(b.folder ?? [])].join(' ').toLocaleLowerCase('ru').includes(query));
  const rows = bookmarks.map((b) => collectionItem(
    b, () => navigate(b.url), () => run('remove_bookmark_by_id', { id: b.id }), 'Удалить закладку'
  ));
  $('bookmarks').replaceChildren(...(rows.length ? rows : [toText('p', 'muted', query ? 'Ничего не найдено.' : 'Здесь появятся сохранённые сайты.')]));
}
$('bookmark-filter').addEventListener('input', renderBookmarks);

function bookmarkButton(item, inFolder = false) {
  const label = inFolder && item.folder.length > 1 ? item.folder.slice(1).join(' / ') + ' · ' + item.title : item.title;
  const node = toText('button', 'bookmark-chip', '');
  node.type = 'button';
  node.title = item.title + ' · ' + item.url;
  node.append(favicon(item.url, item.title, null, item), toText('span', 'bookmark-label', label));
  node.addEventListener('click', () => navigate(item.url));
  node.addEventListener('auxclick', e => {
    if (e.button === 1) run('add_tab', { url: item.url, private: state.private });
  });
  return node;
}
function renderBookmarkStrip() {
  const children = [];
  const folders = new Map();
  for (const item of state.bookmarks) {
    const name = item.folder?.[0];
    if (!name) {
      children.push(bookmarkButton(item));
      continue;
    }
    if (folders.has(name)) {
      folders.get(name).push(item);
      continue;
    }
    const items = [item];
    folders.set(name, items);
    const folder = button(name, 'folder', 'bookmark-chip', () => {
      const reopens = bookmarkMenu.hidden || bookmarkAnchor !== folder;
      closeLayer();
      if (reopens) {
        bookmarkAnchor = folder;
        bookmarkMenu.style.left = Math.max(8, Math.min(folder.getBoundingClientRect().left, innerWidth - 322)) + 'px';
        $('bookmark-menu-items').replaceChildren(...items.map(entry => bookmarkButton(entry, true)));
        bookmarkMenu.hidden = false;
        $('bookmark-menu-items').firstElementChild?.focus({ preventScroll: true });
      }
      setLayer();
    });
    folder.append(toText('span', 'bookmark-label', name));
    children.push(folder);
  }
  $('bookmark-strip').replaceChildren(...children);
}
$('bookmark-library').addEventListener('click', () => openPanel('bookmarks'));
$('bookmark-strip').addEventListener('wheel', e => {
  const track = e.currentTarget;
  if (track.scrollWidth > track.clientWidth && Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
    e.preventDefault();
    track.scrollLeft += e.deltaY;
  }
}, { passive: false });

async function loadPlugins() {
  const records = await run('plugins_list');
  if (!Array.isArray(records)) return;
  $('plugins').replaceChildren(...records.map((record) => {
    const { manifest, enabled } = record;
    const row = document.createElement('article');
    row.className = 'item plugin';
    const text = document.createElement('div');
    text.className = 'item-text';
    const edit = toText('button', 'pill', 'Редактировать код');
    edit.type = 'button';
    edit.addEventListener('click', async () => {
      const code = await run('plugin_get_code', { id: manifest.id });
      if (!code) return;
      editorId = manifest.id;
      $('plugin-name').textContent = manifest.name;
      $('plugin-css').value = code[0];
      $('plugin-js').value = code[1];
      $('plugin-hosts').value = (record.hosts ?? []).join(', ');
      $('plugin-editor').hidden = false;
      $('plugin-editor').scrollIntoView({ block: 'nearest' });
    });
    text.append(toText('strong', 'item-title', manifest.name), toText('span', 'item-subtitle', manifest.description), edit);
    const toggle = document.createElement('input');
    toggle.className = 'switch';
    toggle.type = 'checkbox';
    toggle.checked = enabled;
    toggle.setAttribute('aria-label', manifest.name);
    toggle.addEventListener('change', async () => {
      toggle.disabled = true;
      if (await run('plugin_set_enabled', { id: manifest.id, enabled: toggle.checked })) {
        toast('Обновите нужную вкладку, чтобы применить изменения');
      } else {
        toggle.checked = !toggle.checked;
      }
      toggle.disabled = false;
    });
    row.append(text, toggle);
    return row;
  }));
}
$('plugins-refresh').addEventListener('click', loadPlugins);
$('plugins-folder').addEventListener('click', () => run('plugins_open_folder'));
$('plugin-save').addEventListener('click', async () => {
  if (await run('plugin_set_code', { id: editorId, css: $('plugin-css').value, js: $('plugin-js').value, hosts: $('plugin-hosts').value })) {
    toast('Код сохранён. Обновите нужную вкладку');
  }
});

function confirmAction(title, explanation, label) {
  if (confirmDialog.open) return Promise.resolve(false);
  $('confirm-title').textContent = title;
  $('confirm-text').textContent = explanation;
  $('confirm-action').textContent = label;
  confirmDialog.returnValue = '';
  return new Promise((resolve) => {
    confirmDialog.addEventListener('close', () => resolve(confirmDialog.returnValue === 'accept'), { once: true });
    confirmDialog.showModal();
  });
}
$('export').addEventListener('click', async () => {
  const data = await run('export_data');
  if (!data) return;
  $('data-io').value = data;
  if (fileExportUrl) URL.revokeObjectURL(fileExportUrl);
  fileExportUrl = URL.createObjectURL(new Blob([data], { type: 'application/json' }));
  const download = document.createElement('a');
  download.href = fileExportUrl;
  download.download = 'aurora-settings.json';
  download.click();
});
$('choose-import').addEventListener('click', () => $('import-file').click());
$('import-file').addEventListener('change', async (e) => {
  const file = e.currentTarget.files[0];
  e.currentTarget.value = '';
  if (!file) return;
  if (file.size > 8 * 1024 * 1024) return toast('Допустимый размер файла — до 8 МБ');
  try { $('data-io').value = await file.text(); } catch { toast('Не удалось прочитать файл'); }
});
$('import').addEventListener('click', async () => {
  const payload = $('data-io').value.trim();
  if (!payload) return toast('Откройте файл или вставьте JSON');
  if (!await confirmAction('Импортировать данные?', 'Настройки и закладки из файла заменят соответствующие данные Aurora.', 'Импортировать')) return;
  await settingsQueue;
  const result = await run('import_data', { payload });
  if (result) {
    await resync();
    toast(result);
  }
});
$('clear').addEventListener('click', async () => {
  if (await confirmAction('Очистить историю?', 'Из списка будут удалены все посещения.', 'Очистить') && await run('history_clear')) {
    toast('История очищена');
  }
});

async function toggleBookmark() {
  const bookmarked = await run('toggle_bookmark');
  if (bookmarked !== null) toast(bookmarked ? 'Закладка добавлена' : 'Закладка удалена');
}
async function windowAction(action) {
  await run('window_ctl', { action });
  syncWindow();
}
async function syncWindow() {
  const result = await run('window_state');
  if (!result) return;
  const maximize = document.querySelector('[data-window="maximize"]');
  maximize.replaceChildren(icon(result.maximized ? 'restore' : 'square'));
  maximize.title = result.maximized ? 'Восстановить размер окна' : 'Развернуть';
}
function dispatch(action) {
  if (action === 'address') return focusAddress();
  if (action === 'newtab') return newTab();
  if (action === 'private') return newTab(true);
  if (action === 'close') return closeTab(state.activeTabId);
  if (action === 'reopen') { closeLayer(); return run('reopen_tab'); }
  if (action === 'bookmark') return toggleBookmark();
  if (action === 'history') return navigate('aurora://history');
  if (action === 'fullscreen') { closeLayer(); return windowAction(action); }
  if (action === 'settings') return currentPanel === 'settings' ? dismissLayer() : openPanel(action);
  if (action === 'tab-list') return openPanel('tabs');
  if (action === 'bookmarks') return openPanel(action);
  if (action === 'transfer') return openPanel(action);
  if (action === 'profiles') return openPanel(action);
  if (action === 'downloads') return toggleDownloads();
  if (action === 'next-tab' || action === 'previous-tab') return cycle(action === 'next-tab' ? 1 : -1);
  if (action.startsWith('tab-')) {
    const index = Number(action.slice(4)) - 1;
    const target = index === 8 ? state.tabs.at(-1) : state.tabs[index];
    if (target) return selectTab(target.id);
    return;
  }
  if (!action.startsWith('zoom-')) closeLayer();
  return run('tab_action', { action });
}

$('menu-button').addEventListener('click', (event) => {
  const opening = menu.hidden;
  closeLayer();
  menu.hidden = !opening;
  setLayer();
  if (opening && event.detail === 0) menu.querySelector('button').focus({ preventScroll: true });
});
$('newtab').addEventListener('click', () => newTab());
$('brand').addEventListener('click', () => dispatch('home'));
$('bookmark').addEventListener('click', toggleBookmark);
$('tab-list').addEventListener('click', () => openPanel('tabs'));
$('panel-close').addEventListener('click', dismissLayer);
$('downloads-close').addEventListener('click', dismissLayer);
$('downloads-folder').addEventListener('click', () => run('open_downloads_folder'));
backdrop.addEventListener('click', dismissLayer);
panel.addEventListener('click', (e) => { if (e.target === panel) dismissLayer(); });
document.addEventListener('click', (e) => {
  const item = e.target.closest('button');
  if (!item || item.disabled) return;
  if (item.dataset.view) openPanel(item.dataset.view);
  if (item.dataset.action) dispatch(item.dataset.action);
  if (item.dataset.command) dispatch(item.dataset.command);
  if (item.dataset.window) windowAction(item.dataset.window);
});
chrome.addEventListener('pointerdown', (e) => {
  if (e.button === 0 && !tabContext.hidden) closeLayer();
  if (e.button !== 0 || !e.target.closest('[data-drag], .toolbar')) return;
  if (e.target.closest('button, input, form, .tab, #compact-window-slot')) return;
  if (e.detail === 2) windowAction('maximize');
  else run('win_drag');
});
document.addEventListener('keydown', (e) => {
  if (confirmDialog.open) return;
  if (e.key === 'Escape') {
    if (!bookmarkMenu.hidden || !tabContext.hidden || !suggest.hidden || !panel.hidden || !menu.hidden || !downloads.hidden) dismissLayer();
    else if (document.activeElement === address) address.blur();
    return;
  }
  const ctrl = e.ctrlKey || e.metaKey;
  const key = e.key.toLowerCase();
  let action;
  if (ctrl && !e.altKey) {
    action = {
      l: 'address', t: e.shiftKey ? 'reopen' : 'newtab', w: 'close', r: 'reload', d: 'bookmark', h: 'history',
      n: e.shiftKey ? 'private' : undefined,
      j: 'downloads', ',': 'settings', tab: e.shiftKey ? 'previous-tab' : 'next-tab',
      '+': 'zoom-in', '=': 'zoom-in', '-': 'zoom-out', '0': 'zoom-reset',
      i: e.shiftKey ? 'devtools' : undefined,
    }[key];
    if (/^[1-9]$/.test(key)) action = 'tab-' + key;
  } else if (e.altKey) {
    action = { d: 'address', arrowleft: 'back', arrowright: 'forward' }[key];
  } else {
    action = { f5: 'reload', f11: 'fullscreen', f12: 'devtools' }[key];
  }
  if (action) {
    e.preventDefault();
    if (!e.repeat) dispatch(action);
    return;
  }
  if (key === 'tab' && currentPanel) {
    const buttons = Array.from(panel.querySelectorAll('button, input, select, textarea'))
      .filter((node) => !node.disabled && node.getClientRects().length);
    const from = e.shiftKey ? buttons[0] : buttons.at(-1);
    if (document.activeElement === from) {
      e.preventDefault();
      (e.shiftKey ? buttons.at(-1) : buttons[0])?.focus({ preventScroll: true });
    }
  }
});

window.addEventListener('focus', syncWindow);
let chromeWidth = window.innerWidth;
window.addEventListener('resize', () => {
  if (window.innerWidth === chromeWidth) return;
  chromeWidth = window.innerWidth;
  if (!tabContext.hidden) closeLayer();
  if (suggestionRows.length) hideSuggestions();
  liquidGlass.setPage(glassPage().key);
  if (currentOverlay) setLayer();
});
async function init() {
  if (!core || !event) return toast('Не удалось подключиться к ядру Aurora');
  try {
    await Promise.all([
      event.listen('au://state', scheduleSync),
      event.listen('au://notice', (e) => toast(e.payload)),
      event.listen('au://downloads', refreshDownloads),
      event.listen('au://shortcut', (e) => dispatch(e.payload)),
    ]);
  } catch (error) {
    toast(error);
  }
  await resync();
  syncWindow();
}
init();
