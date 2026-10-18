'use strict';
// Progressive enhancement only: every download link works without JavaScript.
(() => {
  const ru = document.documentElement.lang === 'ru';

  // Make the visitor's own platform the primary download. Other systems keep Mac first.
  const ua = navigator.userAgent;
  const platform = /Windows/i.test(ua) ? 'windows' : 'mac';
  for (const button of document.querySelectorAll('.download-actions .button[data-platform], .platform .button[data-platform]')) {
    const own = button.dataset.platform === platform;
    button.classList.toggle('primary', own);
    button.classList.toggle('secondary', !own);
  }
  const actions = document.querySelector('.download-actions');
  const ownButton = actions && actions.querySelector(`[data-platform="${platform}"]`);
  if (ownButton) actions.prepend(ownButton);
  const platforms = document.querySelector('.platforms');
  const ownCard = platforms && platforms.querySelector(`[data-platform-card="${platform}"]`);
  if (ownCard) platforms.prepend(ownCard);

  // Same-origin release metadata; never a node/bootstrap/configuration service.
  const format = bytes => `${(bytes / 1048576).toLocaleString(ru ? 'ru-RU' : 'en-US', { maximumFractionDigits: 1 })} ${ru ? 'МБ' : 'MB'}`;
  fetch(new URL('release.json', document.currentScript.src), { cache: 'no-cache', redirect: 'error' })
    .then(response => response.ok ? response.json() : null)
    .then(release => {
      if (!release) return;
      const sizes = { exe: release.exe_bytes, mac: release.macos_zip_bytes };
      for (const node of document.querySelectorAll('[data-size]')) {
        const bytes = sizes[node.dataset.size];
        if (Number.isSafeInteger(bytes) && bytes > 0) node.textContent = format(bytes);
      }
    }).catch(() => { /* Downloads and factual copy work without JavaScript. */ });
})();
