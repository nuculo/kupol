'use strict';
const copyButton = document.querySelector('[data-copy]');
const status = document.querySelector('.copy-status');
copyButton.addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(document.getElementById('install-commands').textContent.trim());
    status.textContent = 'Installation commands copied.';
    copyButton.textContent = 'Copied ✓';
  } catch {
    status.textContent = 'Select the commands above and copy them manually.';
  }
});
