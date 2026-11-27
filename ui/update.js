'use strict';

function startUpdateUi() {
  if (!state.meta?.updates) return;

  const indicator = $('update-button');
  const badge = $('update-badge-text');
  const menuButton = $('update-menu-button');
  const details = $('update-version');
  const notes = $('update-notes');
  const status = $('update-status');
  const progress = $('update-progress');
  const checkButton = $('update-check-again');
  const installButton = $('update-install');
  let available;
  let outcome;
  let checking = false;
  let installing = false;
  let restarting = false;
  let checked = false;
  let percent = 0;
  let error = '';
  let finishedTimer;

  function textBadge(kind, text) {
    indicator.hidden = !kind;
    if (!kind) return;
    if (indicator.dataset.kind !== kind) {
      indicator.dataset.kind = kind;
      indicator.firstElementChild.replaceWith(icon(kind === 'complete' ? 'check' : 'update'));
    }
    badge.textContent = text;
    indicator.title = text;
    indicator.setAttribute('aria-label', text);
  }

  function render() {
    if (restarting) textBadge('loading', 'Перезапуск Aurora…');
    else if (installing) textBadge('loading', 'Загрузка · ' + percent + '%');
    else if (outcome) textBadge(outcome.error ? 'error' : 'complete', outcome.error ? 'Не удалось обновиться' : 'Обновление завершено');
    else if (available) textBadge('available', 'Доступно обновление');
    else textBadge('', '');

    details.hidden = !(available || outcome);
    details.textContent = available ? 'Aurora ' + available.version : outcome ? 'Aurora ' + outcome.version : '';
    notes.textContent = available?.notes ?? '';
    notes.hidden = !notes.textContent;
    status.dataset.error = String(Boolean(error || outcome?.error));
    status.textContent = restarting ? 'Сохраняем сеанс и перезапускаем браузер…'
      : installing ? 'Загружено ' + percent + '%'
      : error || outcome?.error || (checking ? 'Проверяем релизы GitHub…'
        : outcome ? 'Обновление завершено'
        : available ? 'Браузер перезапустится автоматически.'
        : checked ? 'Установлена актуальная версия ' + state.meta.version + '.'
        : 'Установлена версия ' + state.meta.version + '.');
    progress.hidden = !installing;
    progress.value = percent;
    checkButton.disabled = checking || installing;
    installButton.hidden = !available;
    installButton.disabled = checking || installing;
  }

  async function check(manual = false) {
    if (checking || installing) return;
    checking = true;
    if (manual) {
      outcome = null;
      error = '';
    }
    render();
    try {
      available = await core.invoke('update_check');
      checked = true;
      error = '';
    } catch (problem) {
      if (manual || !updatePanel.hidden) error = String(problem?.message ?? problem).slice(0, 320);
    } finally {
      checking = false;
      render();
    }
  }

  function showPanel() {
    if (!updatePanel.hidden) return dismissLayer();
    closeLayer();
    updatePanel.hidden = false;
    render();
    setLayer();
    run('focus_chrome').then(() => {
      if (!updatePanel.hidden) $('update-close').focus({ preventScroll: true });
    });
  }
  indicator.addEventListener('click', showPanel);
  menuButton.addEventListener('click', () => {
    showPanel();
    if (!available && !outcome) check(true);
  });
  $('update-close').addEventListener('click', dismissLayer);
  checkButton.addEventListener('click', () => check(true));
  installButton.addEventListener('click', async () => {
    if (!available || installing) return;
    error = '';
    outcome = null;
    installing = true;
    percent = 0;
    render();
    try {
      await core.invoke('update_install', { version: available.version });
    } catch (problem) {
      installing = false;
      restarting = false;
      error = String(problem?.message ?? problem).slice(0, 320);
      render();
    }
  });

  async function listen() {
    await event.listen('au://update-progress', ({ payload }) => {
      if (installing) {
        percent = payload;
        render();
      }
    });
    await event.listen('au://update-restarting', () => {
      restarting = true;
      render();
    });
    menuButton.hidden = false;
    try {
      outcome = await core.invoke('update_last_result');
      if (outcome && !outcome.error) {
        clearTimeout(finishedTimer);
        finishedTimer = setTimeout(() => {
          outcome = null;
          render();
        }, 9000);
      }
      render();
    } catch {
      // The manual check is still available if there was no readable result.
    }
    setTimeout(check, 1800);
    setInterval(check, 6 * 60 * 60 * 1000);
  }
  listen().catch(error => console.warn('Обновления Aurora', error));
}
