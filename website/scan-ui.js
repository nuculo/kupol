'use strict';
/*
 * Wiring for the in-browser "Scan your code" section.
 * Uses window.KUPOL (scanner.js). Everything runs client-side.
 */
(function () {
  const $ = (sel) => document.querySelector(sel);

  const textarea = $('#scan-input');
  const filenameInput = $('#scan-filename');
  const fileInput = $('#scan-file');
  const runBtn = $('#scan-run');
  const sampleBtn = $('#scan-sample');
  const clearBtn = $('#scan-clear');
  const dropzone = $('#scan-dropzone');
  const resultsEl = $('#scan-results');
  const statusEl = $('#scan-status');

  if (!textarea || !runBtn || !window.KUPOL) return;

  const MAX_BYTES = 200 * 1024; // 200 KB per file, demo-safe
  const MAX_FILES = 12;

  // Files added via upload/drop. Pasted code is handled separately.
  let uploadedFiles = [];

  const SAMPLE = {
    name: 'example.rs',
    content: `// KUPOL sample — intentionally vulnerable
use std::collections::HashMap;

fn lookup_user(name: &str) -> String {
    // SQL built by string interpolation
    format!("SELECT * FROM users WHERE name = {}", name)
}

const API_KEY: &str = "sk-live-not-a-real-secret-demo";

fn hash_password(p: &str) -> String {
    // weak hashing
    MD5(p)
}

fn parse_port(s: &str) -> u16 {
    s.parse().unwrap()
}

// TODO: security — validate the redirect target before shipping
fn redirect(url: &str) {
    unsafe {
        do_jump(url);
    }
}
`,
  };

  const PY_SAMPLE = {
    name: 'app.py',
    content: `import os

def run(cmd):
    # dangerous API
    os.system("ping " + cmd)

API_KEY = "sk-live-demo-key-not-real"

def render(user_html):
    el.innerHTML = user_html  # DOM injection
`,
  };

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, (c) => (
      { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]
    ));
  }

  function setStatus(msg) {
    if (statusEl) statusEl.textContent = msg || '';
  }

  function guessExtension(code) {
    if (/\bfn\s+\w+|let\s+mut|println!|unwrap\(\)|::/.test(code)) return '.rs';
    if (/\bdef\s+\w+|import\s+\w+|os\.system|print\(/.test(code)) return '.py';
    if (/\bfunction\b|=>|const\s+\w+\s*=|console\.log/.test(code)) return '.js';
    return '.txt';
  }

  function collectFiles() {
    const files = [];
    // Uploaded files first.
    for (const f of uploadedFiles) files.push(f);
    // Pasted code (if any).
    const pasted = textarea.value;
    if (pasted.trim()) {
      let name = (filenameInput.value || '').trim();
      if (!name) name = `pasted${guessExtension(pasted)}`;
      files.push({ name, content: pasted });
    }
    return files;
  }

  function renderResults({ findings, summary }, fileCount) {
    if (!findings.length) {
      resultsEl.innerHTML = `
        <div class="scan-result-head">
          <span class="result-ok">✓ No findings</span>
          <span>${fileCount} file${fileCount === 1 ? '' : 's'} scanned</span>
        </div>
        <p class="preview-caption">No matches from the built-in checks. Try the sample, or paste code that uses a risky pattern.</p>`;
      return;
    }

    const label = window.KUPOL.riskLabel(summary.riskScore);
    const chips = [];
    if (summary.critical) chips.push(`<span class="chip crit">${summary.critical} critical</span>`);
    if (summary.high) chips.push(`<span class="chip high">${summary.high} high</span>`);
    if (summary.medium) chips.push(`<span class="chip">${summary.medium} medium</span>`);
    if (summary.low) chips.push(`<span class="chip">${summary.low} low</span>`);
    if (summary.info) chips.push(`<span class="chip">${summary.info} info</span>`);

    const rows = findings.map((f) => {
      const sevClass = (f.severity === 'critical' || f.severity === 'high') ? 'severity' : 'severity medium';
      const loc = f.line ? `${f.file}:${f.line}` : f.file;
      const cwe = f.cwe ? `<small>${escapeHtml(f.cwe)}</small>` : '';
      return `
        <div class="scan-row" role="row">
          <span role="cell"><b class="${sevClass}">${f.severity.toUpperCase()}</b></span>
          <span role="cell">${escapeHtml(f.title)}<small>${escapeHtml(f.plugin)}</small></span>
          <code role="cell">${escapeHtml(loc)}</code>
        </div>`;
    }).join('');

    resultsEl.innerHTML = `
      <div class="scan-result-head">
        <span>Risk score <b class="risk-${label.toLowerCase()}">${summary.riskScore.toFixed(1)} / 10 · ${label}</b></span>
        <span class="scan-chips">${chips.join('')}</span>
      </div>
      <div class="scan-table" role="table" aria-label="Scan findings">
        <div class="scan-row scan-head" role="row">
          <span role="columnheader">SEVERITY</span>
          <span role="columnheader">FINDING</span>
          <span role="columnheader">LOCATION</span>
        </div>
        ${rows}
      </div>
      <p class="preview-caption">${summary.totalFindings} finding${summary.totalFindings === 1 ? '' : 's'} across ${fileCount} file${fileCount === 1 ? '' : 's'}. Runs entirely in your browser — nothing is uploaded. The CLI runs the same checks over a whole repo.</p>`;
  }

  function runScan() {
    const files = collectFiles();
    if (!files.length) {
      setStatus('Paste some code or add a file first.');
      resultsEl.innerHTML = '';
      return;
    }
    setStatus('Scanning…');
    // Defer so the status paints before the (fast) scan.
    setTimeout(() => {
      try {
        const result = window.KUPOL.scan(files);
        renderResults(result, files.length);
        setStatus(`Done — ${result.summary.totalFindings} finding${result.summary.totalFindings === 1 ? '' : 's'}.`);
      } catch (e) {
        setStatus('Could not scan that input.');
        resultsEl.innerHTML = `<p class="preview-caption">Error: ${escapeHtml(e.message || String(e))}</p>`;
      }
    }, 20);
  }

  function addFiles(fileList) {
    const items = Array.from(fileList || []);
    let skipped = 0;
    for (const file of items) {
      if (uploadedFiles.length >= MAX_FILES) { skipped++; continue; }
      if (file.size > MAX_BYTES) { skipped++; continue; }
      const reader = new FileReader();
      reader.onload = () => {
        uploadedFiles.push({ name: file.name, content: String(reader.result || '') });
        renderFileChips();
      };
      reader.readAsText(file);
    }
    if (skipped) setStatus(`${skipped} file(s) skipped (over ${MAX_FILES} files or ${MAX_BYTES / 1024} KB each).`);
  }

  function renderFileChips() {
    const chipsHost = $('#scan-filechips');
    if (!chipsHost) return;
    if (!uploadedFiles.length) { chipsHost.innerHTML = ''; return; }
    chipsHost.innerHTML = uploadedFiles.map((f, idx) =>
      `<span class="file-chip">${escapeHtml(f.name)} <button type="button" data-remove="${idx}" aria-label="Remove ${escapeHtml(f.name)}">×</button></span>`
    ).join('');
  }

  // ── Events ───────────────────────────────────────────────────────────────
  runBtn.addEventListener('click', runScan);

  if (sampleBtn) {
    sampleBtn.addEventListener('click', () => {
      textarea.value = SAMPLE.content;
      filenameInput.value = SAMPLE.name;
      uploadedFiles = [{ ...PY_SAMPLE }];
      renderFileChips();
      setStatus('Loaded a sample Rust file + a Python file. Hit "Scan".');
      runScan();
    });
  }

  if (clearBtn) {
    clearBtn.addEventListener('click', () => {
      textarea.value = '';
      filenameInput.value = '';
      uploadedFiles = [];
      renderFileChips();
      resultsEl.innerHTML = '';
      setStatus('');
    });
  }

  if (fileInput) {
    fileInput.addEventListener('change', (e) => { addFiles(e.target.files); e.target.value = ''; });
  }

  const chipsHost = $('#scan-filechips');
  if (chipsHost) {
    chipsHost.addEventListener('click', (e) => {
      const btn = e.target.closest('[data-remove]');
      if (!btn) return;
      uploadedFiles.splice(Number(btn.dataset.remove), 1);
      renderFileChips();
    });
  }

  if (dropzone) {
    ['dragenter', 'dragover'].forEach((ev) =>
      dropzone.addEventListener(ev, (e) => { e.preventDefault(); dropzone.classList.add('is-drag'); }));
    ['dragleave', 'drop'].forEach((ev) =>
      dropzone.addEventListener(ev, (e) => { e.preventDefault(); dropzone.classList.remove('is-drag'); }));
    dropzone.addEventListener('drop', (e) => {
      if (e.dataTransfer && e.dataTransfer.files) addFiles(e.dataTransfer.files);
    });
  }
})();
