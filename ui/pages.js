'use strict';

const startSearch = document.getElementById('start-search');
if (startSearch) {
  const query = document.getElementById('query');
  const error = document.getElementById('search-error');
  const submit = startSearch.querySelector('button');
  startSearch.addEventListener('submit', async (event) => {
    event.preventDefault();
    if (submit.disabled || !query.value.trim()) return;
    submit.disabled = true;
    error.hidden = true;
    try {
      const url = new URL(startSearch.action);
      url.searchParams.set('q', query.value);
      const response = await fetch(url);
      if (!response.ok) throw new Error();
      const target = await response.json();
      location.assign(target.url);
    } catch {
      error.textContent = 'Не удалось начать поиск. Попробуйте ещё раз.';
      error.hidden = false;
    } finally {
      submit.disabled = false;
    }
  });
}

const historyList = document.getElementById('history-list');
if (historyList) {
  const time = new Intl.DateTimeFormat('ru', { hour: '2-digit', minute: '2-digit' });
  const date = new Intl.DateTimeFormat('ru', { dateStyle: 'long' });
  const filter = document.getElementById('history-query');
  const message = document.getElementById('history-message');
  const clear = document.getElementById('clear-history');
  const dialog = document.getElementById('clear-dialog');
  const token = document.body.dataset.token;
  let activeDate = '';
  let group;

  for (const item of [...historyList.children]) {
    const timestamp = new Date(Number(item.dataset.time) * 1000);
    if (!Number.isFinite(timestamp.valueOf())) {
      item.remove();
      continue;
    }
    const formatted = date.format(timestamp);
    if (formatted !== activeDate) {
      activeDate = formatted;
      group = document.createElement('section');
      group.className = 'history-group';
      const heading = document.createElement('h2');
      heading.textContent = formatted;
      group.append(heading);
      historyList.append(group);
    }
    const timeEl = item.querySelector('time');
    timeEl.dateTime = timestamp.toISOString();
    timeEl.textContent = time.format(timestamp);
    group.append(item);
  }

  function update() {
    let hasRows = false;
    for (const block of historyList.children) {
      block.hidden = !block.querySelector('.history-row');
      hasRows ||= !block.hidden;
    }
    message.hidden = hasRows;
    message.textContent = filter.defaultValue.trim() ? 'Ничего не найдено.' : 'Здесь появятся посещённые сайты.';
  }

  async function perform(action, id) {
    try {
      const result = await fetch('/do', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ token, action, id })
      });
      if (!result.ok) throw new Error();
      return true;
    } catch {
      message.hidden = false;
      message.textContent = 'Не удалось обновить историю. Повторите попытку.';
      return false;
    }
  }

  historyList.addEventListener('click', async (event) => {
    const button = event.target.closest('button[data-remove]');
    if (!button) return;
    button.disabled = true;
    if (await perform('remove-history', button.dataset.remove)) {
      button.closest('article').remove();
      update();
    } else {
      button.disabled = false;
    }
  });

  clear.addEventListener('click', () => {
    dialog.returnValue = '';
    dialog.showModal();
  });
  dialog.addEventListener('close', async () => {
    if (dialog.returnValue === 'clear' && await perform('clear-history')) {
      historyList.replaceChildren();
      document.getElementById('history-pagination').replaceChildren();
      clear.disabled = true;
      update();
    }
  });
  update();
}
