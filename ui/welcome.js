'use strict';

function startWelcome() {
  const wizard = $('welcome');
  const steps = [...wizard.querySelectorAll('.welcome-step')];
  const progress = [...wizard.querySelectorAll('.welcome-steps li')];
  const buttons = { back: $('welcome-back'), skip: $('welcome-skip'), next: $('welcome-next') };
  const source = $('welcome-source');
  const importButton = $('welcome-import');
  const retention = $('welcome-retention');
  const importStatus = $('welcome-import-status');
  const errorStatus = $('welcome-error');
  const defaults = state.settings;
  const initialPreferences = {
    theme: defaults.theme,
    search_engine: defaults.search_engine,
    show_bookmark_bar: defaults.show_bookmark_bar,
    animated_wallpaper: defaults.animated_wallpaper,
  };
  const preferences = { ...initialPreferences };
  const entries = [];
  const selectedFields = new Set(['tabs', 'bookmarks', 'history']);
  let current = 0;
  let sources = [];
  let scanned = false;
  let scanning = false;
  let busy = false;

  function setStatus(element, message = '', error = false) {
    element.textContent = message;
    element.hidden = !message;
    element.toggleAttribute('data-error', error);
  }

  function updateControls() {
    const profile = sources.find(item => item.id === source.value);
    source.disabled = busy || !sources.length;
    for (const [key, field] of entries) {
      field.disabled = busy || !profile?.[key];
      field.checked = Boolean(profile?.[key]) && selectedFields.has(key);
    }
    const history = entries.find(([key]) => key === 'history')?.[1];
    $('welcome-retention-row').hidden = !profile?.history;
    retention.disabled = busy || !history?.checked;
    importButton.disabled = !entries.some(([, field]) => field.checked && !field.disabled);
    for (const id of ['welcome-html', 'welcome-csv']) $(id).disabled = busy;
    for (const button of Object.values(buttons)) button.disabled = busy;
  }

  async function scan() {
    if (scanning) return;
    scanning = true;
    source.replaceChildren(new Option('Поиск браузеров…', ''));
    setStatus(importStatus);
    try {
      sources = await core.invoke('browser_sources');
      const options = sources.map(item => new Option(item.browser + ' · ' + item.profile, item.id));
      source.replaceChildren(...(options.length ? options : [new Option('Браузеры не найдены', '')]));
      scanned = true;
    } catch {
      source.replaceChildren(new Option('Поиск не удался', ''));
      if (current === 2) setStatus(importStatus, 'Вернитесь на предыдущий шаг и снова откройте перенос или выберите файл.', true);
    } finally {
      scanning = false;
      updateControls();
    }
  }

  function showStep(index) {
    current = index;
    setStatus(errorStatus);
    for (const [i, step] of steps.entries()) {
      step.hidden = i !== index;
      if (i === index) progress[i].setAttribute('aria-current', 'step');
      else progress[i].removeAttribute('aria-current');
    }
    buttons.back.hidden = index === 0;
    buttons.skip.hidden = index === 2;
    buttons.skip.textContent = index === 0 ? 'Пропустить настройку' : 'Пропустить перенос';
    buttons.next.textContent = ['Начать', 'Далее', 'Открыть Aurora'][index];
    wizard.querySelector('.welcome-scroll').scrollTop = 0;
    steps[index].querySelector('h1').focus({ preventScroll: true });
    if (index === 2 && !scanned) scan();
  }

  async function finish(values) {
    if (busy) return;
    busy = true;
    setStatus(errorStatus);
    updateControls();
    try {
      await core.invoke('finish_onboarding', { preferences: values });
    } catch (error) {
      busy = false;
      updateControls();
      setStatus(errorStatus, error?.message ?? String(error), true);
      return;
    }
    wizard.hidden = true;
    document.body.classList.remove('welcoming');
    chrome.inert = false;
    await resync();
    $('menu-button').focus({ preventScroll: true });
    startUpdateUi();
  }

  async function withImport(message, operation) {
    if (busy) return;
    busy = true;
    updateControls();
    setStatus(importStatus, message);
    try {
      setStatus(importStatus, await operation());
    } catch (error) {
      setStatus(importStatus, error?.message ?? String(error), true);
    } finally {
      busy = false;
      updateControls();
      buttons.next.focus({ preventScroll: true });
    }
  }

  for (const [key, title] of [
    ['tabs', 'Вкладки'], ['bookmarks', 'Закладки'], ['history', 'История'], ['downloads', 'Журнал загрузок'],
  ]) {
    const label = document.createElement('label');
    const field = document.createElement('input');
    field.type = 'checkbox';
    field.addEventListener('change', () => {
      if (field.checked) selectedFields.add(key);
      else selectedFields.delete(key);
      updateControls();
    });
    label.append(field, title);
    $('welcome-fields').append(label);
    entries.push([key, field]);
  }
  source.addEventListener('change', updateControls);
  importButton.addEventListener('click', () => {
    const fields = Object.fromEntries(entries.map(([key, field]) => [key, field.checked && !field.disabled]));
    fields.keep_history = fields.history && retention.checked;
    withImport('Переносим выбранные данные…', async () => summaryText(await core.invoke('import_browser', { id: source.value, fields })));
  });
  $('welcome-csv').hidden = $('welcome-password-hint').hidden = !isWindows;
  for (const format of Object.keys(migrationFormats)) {
    const button = $('welcome-' + format);
    const input = $('welcome-' + format + '-file');
    button.addEventListener('click', () => input.click());
    input.addEventListener('change', () => {
      const file = input.files[0];
      input.value = '';
      if (!file) return;
      withImport('Читаем файл…', async () => migrationFileSummary(format, await importMigrationFile(format, file)));
    });
  }

  function preview(theme) {
    preferences.theme = theme;
    wizard.dataset.theme = theme;
  }
  preview(defaults.theme);
  for (const field of wizard.querySelectorAll('[name="welcome-theme"]')) {
    field.checked = field.value === defaults.theme;
    field.addEventListener('change', () => preview(field.value));
  }
  for (const [id, key] of [
    ['welcome-engine', 'search_engine'], ['welcome-bookmark-bar', 'show_bookmark_bar'],
    ['welcome-wallpaper-motion', 'animated_wallpaper'],
  ]) {
    const field = $(id);
    if (field.type === 'checkbox') field.checked = preferences[key];
    else field.value = preferences[key];
    field.addEventListener('change', () => {
      preferences[key] = field.type === 'checkbox' ? field.checked : field.value;
    });
  }

  buttons.back.addEventListener('click', () => showStep(current - 1));
  buttons.skip.addEventListener('click', () => finish(current === 0 ? initialPreferences : preferences));
  buttons.next.addEventListener('click', () => current < 2 ? showStep(current + 1) : finish(preferences));
  wizard.querySelector('.welcome-titlebar').addEventListener('pointerdown', e => {
    if (e.button !== 0 || e.target.closest('button')) return;
    if (e.detail === 2) windowAction('maximize');
    else run('win_drag');
  });
  wizard.hidden = false;
  document.body.classList.add('welcoming');
  chrome.inert = true;
  updateControls();
  showStep(0);
}
