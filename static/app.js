// PoliticalDebAIser — Client-side Application (v2)

const ARCHETYPE_META = {
  conservative: { icon: '\u{1F3DB}\uFE0F', label: 'Conservative' },
  democrat:     { icon: '\u{1F5F3}\uFE0F', label: 'Democrat' },
  socialist:    { icon: '\u270A',           label: 'Socialist' },
  dictatorship: { icon: '\u{1F441}\uFE0F', label: 'Dictatorship' },
  anarchist:    { icon: '\u{1F3F4}',        label: 'Anarchist' },
};

// DOM refs
const form          = document.getElementById('analyze-form');
const urlInput      = document.getElementById('url-input');
const analyzeBtn    = document.getElementById('analyze-btn');
const errorBanner   = document.getElementById('error-banner');
const skeletonGrid  = document.getElementById('skeleton-grid');
const articleInfo   = document.getElementById('article-info');
const resultsArea   = document.getElementById('results');
const cardsGrid     = document.getElementById('cards-grid');
const commonalities = document.getElementById('commonalities');
const synthBtn      = document.getElementById('synthesize-btn');
const synthResult   = document.getElementById('synthesis-result');
const synthText     = document.getElementById('synthesis-text');
const copyBtn       = document.getElementById('copy-btn');

// State
let currentAnalyses = null;
let lastUrl = '';

// ── Helpers ──

function show(el)  { el.classList.add('active'); }
function hide(el)  { el.classList.remove('active'); }

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str;
  return d.innerHTML;
}

// ── Error Handling ──

const ERROR_HINTS = {
  'Ollama is unavailable': 'Make sure Ollama is running: run "ollama serve" in a terminal.',
  'Ollama request timed out': 'The model may be loading for the first time. Try again in a moment.',
  'Invalid URL': 'Check the URL format — it should start with http:// or https://.',
  'Empty article content': 'The page was fetched but no readable article text was found. Try a different URL.',
  'Failed to fetch article': 'The URL could not be reached. Check that it is correct and publicly accessible.',
  'Article fetch timed out': 'The remote site took too long to respond. Try again or use a different source.',
  'Page not found': 'The URL returned a 404. The article may have been removed or the URL is incorrect.',
  'Article behind paywall': 'This article is behind a paywall. Try a different source or a non-paywalled link.',
  'Not an HTML page': 'The URL points to a non-HTML resource (e.g., PDF, image). Paste a link to an article page.',
  'Analysis failed': 'The AI analysis encountered an error. Try again in a moment.',
  'No analyses provided': 'No analysis data was available to synthesize. Run an analysis first.',
};

function classifyError(err) {
  // Try to parse structured JSON error from backend
  if (err._parsed) return err._parsed;
  return { title: 'Something went wrong', body: err.message || String(err), hint: null };
}

function showError(errObj) {
  const { title, body, hint } = errObj;
  document.getElementById('error-title').textContent = title;
  document.getElementById('error-body').textContent = body;
  const hintEl = document.getElementById('error-hint');
  if (hint) {
    hintEl.textContent = hint;
    hintEl.style.display = '';
  } else {
    hintEl.style.display = 'none';
  }
  show(errorBanner);
}

function hideError() {
  hide(errorBanner);
}

async function parseApiError(res) {
  const text = await res.text().catch(() => '');
  try {
    const json = JSON.parse(text);
    const title = json.error || 'Request failed';
    const body = json.details || `Server responded with ${res.status}`;
    const hint = ERROR_HINTS[json.error] || null;
    return { _parsed: { title, body, hint } };
  } catch {
    return { _parsed: { title: 'Request failed', body: text || `Server responded with ${res.status}`, hint: null } };
  }
}

// ── Skeleton Loading ──

function buildSkeletonCard() {
  const card = document.createElement('div');
  card.className = 'skeleton-card';
  card.innerHTML = `
    <div class="skeleton-line skeleton-accent"></div>
    <div class="skeleton-header">
      <div class="skeleton-line skeleton-icon"></div>
      <div class="skeleton-line skeleton-title"></div>
    </div>
    <div class="skeleton-line skeleton-text"></div>
    <div class="skeleton-line skeleton-text"></div>
    <div class="skeleton-line skeleton-text"></div>
    <div class="skeleton-line skeleton-bullet"></div>
    <div class="skeleton-line skeleton-bullet"></div>
    <div class="skeleton-line skeleton-bullet"></div>
    <div class="skeleton-line skeleton-bar"></div>
  `;
  return card;
}

function showSkeletons() {
  skeletonGrid.innerHTML = '';
  for (let i = 0; i < 5; i++) {
    skeletonGrid.appendChild(buildSkeletonCard());
  }
  show(skeletonGrid);
}

function hideSkeletons() {
  hide(skeletonGrid);
}

function setLoading(on) {
  analyzeBtn.disabled = on;
  if (on) {
    hideError();
    hide(resultsArea);
    hide(articleInfo);
    hide(synthResult);
    showSkeletons();
  } else {
    hideSkeletons();
  }
}

// ── Card Builder ──

function buildCard(analysis) {
  const key  = analysis.archetype;
  const meta = ARCHETYPE_META[key] || { icon: '\u2753', label: key };
  const pct  = Math.round(analysis.alignment_score * 100);

  const card = document.createElement('div');
  card.className = 'card';
  card.dataset.archetype = key;

  card.innerHTML = `
    <div class="card-accent"></div>
    <div class="card-header">
      <span class="card-icon">${meta.icon}</span>
      <span class="card-title">${meta.label}</span>
    </div>
    <p class="card-summary">${escapeHtml(analysis.summary)}</p>
    <ul class="card-highlights">
      ${analysis.highlights.map(h => `<li>${escapeHtml(h)}</li>`).join('')}
    </ul>
    <div class="score-bar-container">
      <span class="score-label">Alignment</span>
      <div class="score-bar">
        <div class="score-bar-fill" style="width: 0"></div>
      </div>
      <span class="score-value">${pct}%</span>
    </div>
  `;

  // Animate score bar after insert
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      card.querySelector('.score-bar-fill').style.width = pct + '%';
    });
  });

  return card;
}

// ── Render Results ──

function renderResults(data, sourceUrl) {
  // Article info with source URL
  document.getElementById('article-title').textContent = data.article_title;
  const sourceLink = document.getElementById('article-source');
  sourceLink.textContent = sourceUrl;
  sourceLink.href = sourceUrl;
  document.getElementById('article-summary-text').textContent = data.article_summary;
  show(articleInfo);

  // Cards with staggered fade-in
  cardsGrid.innerHTML = '';
  data.analyses.forEach(a => cardsGrid.appendChild(buildCard(a)));

  // Commonalities — prefer server-provided, fall back to client-side detection
  const serverCommon = data.commonalities && data.commonalities.length > 0
    ? data.commonalities
    : findCommonalities(data.analyses);
  if (serverCommon.length > 0) {
    document.getElementById('commonalities-text').textContent = serverCommon.join(' \u00B7 ');
    show(commonalities);
  } else {
    hide(commonalities);
  }

  // Synthesis
  if (data.synthesis) {
    synthText.textContent = data.synthesis;
    show(synthResult);
    synthBtn.style.display = 'none';
  } else {
    hide(synthResult);
    synthBtn.style.display = '';
    synthBtn.disabled = false;
    synthBtn.textContent = 'Synthesize All Perspectives';
  }

  show(resultsArea);
  currentAnalyses = data.analyses;
}

function findCommonalities(analyses) {
  if (analyses.length < 2) return [];
  const sets = analyses.map(a => a.highlights.map(h => h.toLowerCase()));
  const counts = {};
  sets.forEach(hs => {
    const unique = [...new Set(hs)];
    unique.forEach(h => { counts[h] = (counts[h] || 0) + 1; });
  });
  return Object.entries(counts)
    .filter(([, c]) => c >= 2)
    .map(([h]) => h.charAt(0).toUpperCase() + h.slice(1));
}

// ── Copy to Clipboard ──

copyBtn.addEventListener('click', async () => {
  const text = synthText.textContent;
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    copyBtn.classList.add('copied');
    copyBtn.querySelector('.copy-label').textContent = 'Copied';
    setTimeout(() => {
      copyBtn.classList.remove('copied');
      copyBtn.querySelector('.copy-label').textContent = 'Copy';
    }, 2000);
  } catch {
    // Fallback
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    document.execCommand('copy');
    document.body.removeChild(ta);
    copyBtn.classList.add('copied');
    copyBtn.querySelector('.copy-label').textContent = 'Copied';
    setTimeout(() => {
      copyBtn.classList.remove('copied');
      copyBtn.querySelector('.copy-label').textContent = 'Copy';
    }, 2000);
  }
});

// ── API: Analyze ──

async function doAnalyze(url) {
  setLoading(true);
  lastUrl = url;

  try {
    const res = await fetch('/analyze', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ url }),
    });

    if (!res.ok) {
      const err = await parseApiError(res);
      throw err;
    }

    const data = await res.json();
    setLoading(false);
    renderResults(data, url);
  } catch (err) {
    setLoading(false);
    showError(classifyError(err));
  }
}

form.addEventListener('submit', (e) => {
  e.preventDefault();
  const url = urlInput.value.trim();
  if (!url) return;
  doAnalyze(url);
});

// ── Retry Button ──

document.getElementById('retry-btn').addEventListener('click', () => {
  hideError();
  const url = lastUrl || urlInput.value.trim();
  if (url) doAnalyze(url);
});

// ── API: Synthesize ──

synthBtn.addEventListener('click', async () => {
  if (!currentAnalyses) return;
  synthBtn.disabled = true;
  synthBtn.textContent = 'Synthesizing\u2026';

  try {
    const res = await fetch('/synthesize', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ analyses: currentAnalyses }),
    });

    if (!res.ok) {
      const err = await parseApiError(res);
      throw err;
    }

    const data = await res.json();
    synthText.textContent = data.synthesis;
    show(synthResult);
    synthBtn.style.display = 'none';
    // Update commonalities if the synthesis endpoint returned them
    if (data.commonalities && data.commonalities.length > 0) {
      document.getElementById('commonalities-text').textContent = data.commonalities.join(' \u00B7 ');
      show(commonalities);
    }
  } catch (err) {
    showError(classifyError(err));
    synthBtn.disabled = false;
    synthBtn.textContent = 'Synthesize All Perspectives';
  }
});
