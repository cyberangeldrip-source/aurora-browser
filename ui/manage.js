'use strict';

const autostartControl = $('system-autostart');
const defaultBrowserButton = $('default-browser-choose');
const defaultBrowserStatus = $('default-browser-status');
let systemRequest = 0;
let changingAutostart = false;
let configuredAutostart = false;
$('system-settings').hidden = !isWindows;

async function loadSystemIntegration() {
  if (!isWindows || changingAutostart) return;
  const request = ++systemRequest;
  const status = await run('system_integration');
  if (request !== systemRequest) return;
  if (!status) {
    defaultBrowserStatus.textContent = 'Статус Windows недоступен';
    defaultBrowserStatus.dataset.ready = 'false';
    return;
  }
  configuredAutostart = Boolean(status.autostart);
  autostartControl.checked = configuredAutostart;
  autostartControl.disabled = false;
  defaultBrowserStatus.dataset.ready = String(status.default_browser);
  defaultBrowserStatus.textContent = status.default_browser
    ? 'Aurora открывает веб-ссылки'
    : 'Выберите Aurora в настройках Windows';
  defaultBrowserButton.textContent = status.default_browser ? 'Настройки Windows' : 'Назначить';
}

autostartControl.addEventListener('change', async () => {
  changingAutostart = true;
  systemRequest++;
  autostartControl.disabled = true;
  const enabled = autostartControl.checked;
  if (await run('set_autostart', { enabled })) configuredAutostart = enabled;
  autostartControl.checked = configuredAutostart;
  autostartControl.disabled = false;
  changingAutostart = false;
});
defaultBrowserButton.addEventListener('click', async () => {
  defaultBrowserButton.disabled = true;
  await run('default_browser_settings');
  defaultBrowserButton.disabled = false;
});

function submitForm(form, action) {
  form.addEventListener('submit', async event => {
    event.preventDefault();
    const submit = form.querySelector('button[type="submit"]');
    if (submit.disabled) return;
    submit.disabled = true;
    try { await action(); }
    finally { submit.disabled = false; }
  });
}

async function loadProfiles() {
  const info = await run('profiles_list');
  if (!info) return;
  $('profile-list').replaceChildren(...info.profiles.map(profile => {
    const row = toText('div', 'item', '');
    row.append(toText('strong', 'item-text item-title', profile.name));
    if (profile.id === info.current) {
      row.append(toText('span', 'muted', 'Текущий'));
    } else {
      const open = toText('button', 'pill', 'Открыть в новом окне');
      open.addEventListener('click', () => run('profile_open', { id: profile.id }));
      row.append(open);
    }
    return row;
  }));
}
submitForm($('profile-create'), async () => {
  if (await run('profile_create', { name: $('profile-name').value })) {
    $('profile-create').reset();
    await loadProfiles();
  }
});

let vaultRecords = [];
function renderVault() {
  const query = $('vault-filter').value.trim().toLowerCase();
  const visible = vaultRecords.filter(item => item.some(value => value.toLowerCase().includes(query)));
  $('vault-list').replaceChildren(...visible.map(([site, username]) => {
    const row = toText('div', 'item', '');
    const label = toText('span', 'item-text', '');
    label.append(toText('strong', 'item-title', site), toText('small', 'item-subtitle', username || 'Без имени'));
    const edit = toText('button', 'pill', 'Заменить пароль');
    edit.addEventListener('click', () => {
      $('vault-site').value = site;
      $('vault-user').value = username;
      $('vault-secret').value = '';
      $('vault-secret').focus();
    });
    const remove = button('Удалить сохранённый вход', 'close', 'icon-button', async () => {
      if (await confirmAction('Удалить сохранённый вход?', site, 'Удалить')) {
        const records = await run('password_manage', { action: 'remove', site, username });
        if (Array.isArray(records)) { vaultRecords = records; renderVault(); }
      }
    });
    row.append(label, edit, remove);
    return row;
  }));
  if (!visible.length) $('vault-list').append(toText('p', 'muted', query ? 'Входы не найдены.' : 'Здесь будут сохранённые входы.'));
}
async function loadVault() {
  if (!isWindows) return;
  vaultRecords = await run('password_manage', { action: 'list', site: '', username: '' }) ?? [];
  renderVault();
}
$('vault-panel').hidden = !isWindows;
$('vault-filter').addEventListener('input', renderVault);
submitForm($('vault-save'), async () => {
  const data = { action: 'save', site: $('vault-site').value.trim(), username: $('vault-user').value, password: $('vault-secret').value };
  $('vault-secret').value = '';
  const records = await run('password_manage', data);
  data.password = '';
  if (Array.isArray(records)) {
    $('vault-save').reset();
    vaultRecords = records;
    renderVault();
  }
});

$('browser-extension-card').hidden = !isWindows;
const extensionLoads = new WeakMap();
async function loadBrowserExtensions(list = $('browser-extensions'), quick = false) {
  if (!isWindows) return;
  const request = {};
  extensionLoads.set(list, request);
  const records = await run('browser_extensions', { action: 'list', value: '' });
  if (extensionLoads.get(list) !== request) return;
  if (!Array.isArray(records)) {
    list.replaceChildren(toText('p', 'muted', 'Расширения недоступны в текущем окружении WebView2.'));
    return;
  }
  list.replaceChildren(...records.map(record => {
    const row = toText('div', 'item', '');
    const label = toText('strong', 'item-text item-title', record.name);
    const enabled = document.createElement('input');
    enabled.type = 'checkbox';
    enabled.className = 'switch';
    enabled.checked = record.enabled;
    enabled.dataset.extensionId = record.id;
    enabled.setAttribute('aria-label', record.name);
    enabled.addEventListener('change', async () => {
      const restoreFocus = document.activeElement === enabled;
      enabled.disabled = true;
      await run('browser_extensions', { action: 'enable', value: record.id, enabled: enabled.checked });
      await loadBrowserExtensions(list, quick);
      if (restoreFocus && document.activeElement === document.body) {
        list.querySelector('input[data-extension-id="' + CSS.escape(record.id) + '"]')?.focus();
      }
    });
    row.append(label, enabled);
    if (quick) return row;
    const remove = button('Удалить расширение', 'close', 'icon-button', async () => {
      if (await confirmAction('Удалить расширение?', record.name, 'Удалить')) {
        await run('browser_extensions', { action: 'remove', value: record.id });
        await loadBrowserExtensions();
      }
    });
    row.append(remove);
    return row;
  }));
  if (!records.length) list.append(toText('p', 'muted', 'Установленные расширения появятся здесь.'));
}
submitForm($('extension-install'), async () => {
  const value = $('extension-path').value.trim();
  if (!value) return;
  const accepted = await confirmAction('Добавить расширение?', 'WebView2 применит разрешения из manifest.json. Устанавливайте только знакомые расширения.', 'Добавить');
  if (!accepted) return;
  if (await run('browser_extensions', { action: 'install', value })) {
    $('extension-install').reset();
    await loadBrowserExtensions();
  }
});
