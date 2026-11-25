//! Встроенные плагины Aurora. При первом запуске распаковываются в
//! `<данные>/plugins/<id>/` и ведут себя точно так же, как сторонние:
//! manifest.json + style.css + script.js.

pub const PLUGIN_ADBLOCK_MANIFEST: &str = r#"{
  "id": "adblock-cosmetic",
  "name": "Aurora Adblock",
  "version": "1.0.0",
  "author": "Aurora",
  "description": "Косметическая блокировка рекламы: прячет баннеры, видео-рекламу и трекер-пиксели до их загрузки.",
  "run_on": "web",
  "permissions": ["adblock", "privacy", "cosmetic"]
}"#;

pub const PLUGIN_ADBLOCK_CSS: &str = r#"/* Aurora Adblock — косметический блок-лист */
iframe[src*="doubleclick.net"],
iframe[src*="googlesyndication.com"],
iframe[src*="google_ads"],
iframe[src*="adfox"],
iframe[src*="an.yandex.ru"],
iframe[src*="ads.adfox"],
iframe[src*="yandexadexchange"],
iframe[src*="criteo.com"],
iframe[src*="taboola.com"],
iframe[src*="outbrain.com"],
iframe[src*="amazon-adsystem.com"],
iframe[src*="moatads.com"],
iframe[src*="adsrvr.org"],
iframe[src*="scorecardresearch.com"],
img[src*="doubleclick.net"],
img[src*="googlesyndication.com"],
img[width="1"][height="1"][src*="pixel"],
div[id*="google_ads"],
div[id^="google_ads_"],
div[id^="div-gpt-ad"],
div[class*="adsbygoogle"],
ins.adsbygoogle,
div[id^="yandex_rtb"],
div[class*="yandex-rtb"],
div[id^="adfox_"],
[class*="ad-banner"],
[class*="adbanner"],
[class*="advert"],
[class*="ad-slot"],
[class^="appnexus"],
[class^="criteo"],
[class*="taboola"],
[class*="outbrain"],
[class*="pn-ad"],
[id*="taboola"],
[class*="persistent-ad"],
[class*="promo-ad"],
a[href*="doubleclick.net"],
a[href*="ad.doubleclick"],
a[href*="an.yandex.ru/count"],
a[href*="top-fwz1.mail.ru/goto"],
div[aria-label="Реклама"],
div[data-ad],
div[data-ad-slot],
div[data-adunit],
div[id^="adv-"],
[class^="header-ad"],
[class^="sidebar-ad"],
[class^="footer-ad"],
[class^="ad-container"],
[class^="ad-wrapper"],
[class^="banner-300"],
[id^="banner-"],
object[data*="doubleclick.net"],
embed[src*="doubleclick.net"],
script[src*="doubleclick.net"][async] {
  display: none !important;
  height: 0 !important;
  min-height: 0 !important;
}

/* Убираем «пустые» рекламные каркасы */
div:empty[id*="ad"],
div:empty[class*="ad"] {
  display: none !important;
}
"#;

pub const PLUGIN_ADBLOCK_JS: &str = r#"/* Aurora Adblock — рантайм-скрипт */
(function () {
  'use strict';
  var AD_HOSTS = [
    'doubleclick.net', 'googlesyndication.com', 'googleadservices.com',
    'an.yandex.ru', 'adfox.ru', 'yandexadexchange.net', 'criteo.com',
    'taboola.com', 'outbrain.com', 'amazon-adsystem.com', 'scorecardresearch.com',
    'adsrvr.org', 'moatads.com', 'adnxs.com', 'casalemedia.com', 'pubmatic.com',
    'rubiconproject.com', 'openx.net', 'smartadserver.com', '2mdn.net'
  ];
  function isAd(url) {
    if (!url) return false;
    var u = url.toLowerCase();
    for (var i = 0; i < AD_HOSTS.length; i++) if (u.indexOf(AD_HOSTS[i]) !== -1) return true;
    return false;
  }
  function sweep() {
    try {
      var nodes = document.querySelectorAll('iframe, img, script, embed, object, ins');
      for (var i = 0; i < nodes.length; i++) {
        var n = nodes[i];
        var src = n.src || n.data || (n.getAttribute && n.getAttribute('src')) || '';
        if (isAd(src)) {
          n.remove ? n.remove() : (n.style && (n.style.display = 'none'));
        }
      }
      //腾讯-style «пустые» рекламные контейнеры
      var empty = document.querySelectorAll('div[id*="ad"], div[class*="ad-"], div[class*="adsbygoogle"]');
      for (var j = 0; j < empty.length; j++) {
        var e = empty[j];
        if (!e.textContent || !e.textContent.trim()) e.style.display = 'none';
      }
    } catch (err) { /* тишина */ }
  }
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', sweep, { once: true });
  } else { sweep(); }
  // добиваем то, что дорендерилось позже
  var runs = 0;
  var timer = setInterval(function () {
    sweep();
    if (++runs >= 12) clearInterval(timer);
  }, 750);
})();
"#;

pub const PLUGIN_DARK_MANIFEST: &str = r#"{
  "id": "force-dark",
  "name": "Тёмная тема для всех сайтов",
  "version": "1.0.0",
  "author": "Aurora",
  "description": "Умное затемнение светлых страниц: инвертирует цвета, сохраняя картинки и видео в оригинале.",
  "run_on": "web",
  "permissions": ["cosmetic"]
}"#;

pub const PLUGIN_DARK_CSS: &str = r#"/* Aurora Force Dark */
html {
  background-color: #15161a !important;
  color-scheme: dark !important;
}
html:not([data-aurora-dark-applied]):not([style*="background"]) body {
  background-color: #15161a !important;
}
:root {
  color-scheme: dark !important;
}
"#;

pub const PLUGIN_DARK_JS: &str = r#"/* Aurora Force Dark — аккуратная инверсия */
(function () {
  'use strict';
  if (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches === false) {
    /* светлая системная тема — всё равно применяем, это ручной форс */
  }
  var bg = getComputedStyle(document.documentElement).backgroundColor || '';
  var bodyBg = document.body ? (getComputedStyle(document.body).backgroundColor || '') : '';
  function lum(c) {
    var m = c.match(/(\d+)[,\s]+(\d+)[,\s]+(\d+)/);
    if (!m) return -1;
    return 0.2126 * +m[1] + 0.7152 * +m[2] + 0.0722 * +m[3];
  }
  var l = Math.max(lum(bg), lum(bodyBg));
  if (l >= 0 && l < 120) return; /* страница уже тёмная — не трогаем */

  var s = document.createElement('style');
  s.setAttribute('data-aurora-force-dark', '');
  s.textContent =
    'html{filter:invert(1) hue-rotate(180deg);background:#0b0c0f!important}' +
    'img,video,picture,canvas,svg,iframe,embed,object,[style*="background-image"],[style*="background: url"],' +
    '[style*="background:url"],[data-aurora-no-dark]{filter:invert(1) hue-rotate(180deg)!important}' +
    'meta,content{filter:none}';
  (document.head || document.documentElement).appendChild(s);
  document.documentElement.setAttribute('data-aurora-dark-applied', '1');
})();
"#;

pub const PLUGIN_USER_MANIFEST: &str = r#"{
  "id": "userstyles",
  "name": "Мои стили и скрипты",
  "version": "1.0.0",
  "author": "Ты",
  "description": "Твой CSS и JS, который Aurora вставляет в каждую страницу. Открой код в панели «Плагины» и настрой под себя.",
  "run_on": "web",
  "permissions": ["custom"]
}"#;

pub const PLUGIN_USER_CSS: &str = r#"/* Твой CSS. Примеры — раскомментируй:
body { scroll-behavior: smooth; }
::selection { background: #7C5CFF55; }
*/
"#;

pub const PLUGIN_USER_JS: &str = r#"/* Твой JS (выполняется на каждой странице). Пример:
console.log('[aurora] userscript активен');
*/
"#;

/// Возвращает (id, manifest, css, js) всех встроенных плагинов.
pub fn bundled_plugins() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    vec![
        (
            "adblock-cosmetic",
            PLUGIN_ADBLOCK_MANIFEST,
            PLUGIN_ADBLOCK_CSS,
            PLUGIN_ADBLOCK_JS,
        ),
        (
            "force-dark",
            PLUGIN_DARK_MANIFEST,
            PLUGIN_DARK_CSS,
            PLUGIN_DARK_JS,
        ),
        (
            "userstyles",
            PLUGIN_USER_MANIFEST,
            PLUGIN_USER_CSS,
            PLUGIN_USER_JS,
        ),
    ]
}
