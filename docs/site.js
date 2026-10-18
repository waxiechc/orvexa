'use strict';
// Same-origin release metadata; never a node/bootstrap/configuration service.
fetch('./release.json', { cache: 'no-cache', redirect: 'error' })
  .then(response => response.ok ? response.json() : null)
  .then(release => {
    if (!release || !Number.isSafeInteger(release.exe_bytes) || release.exe_bytes <= 0) return;
    const size = document.getElementById('exe-size');
    if (size) size.textContent = `${(release.exe_bytes / 1048576).toFixed(1)} MB`;
  }).catch(() => { /* Downloads and factual copy work without JavaScript. */ });
