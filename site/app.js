// Debiaser v4 — Client-side Application (Static/Cloudflare Pages edition)
// All LLM calls run in-browser via engine.js; scraping via Cloudflare Worker

// ── DOM refs ──

var form            = document.getElementById('analyze-form');
var urlInput        = document.getElementById('url-input');
var analyzeBtn      = document.getElementById('analyze-btn');
var errorBanner     = document.getElementById('error-banner');
var skeletonGrid    = document.getElementById('skeleton-grid');
var resultsArea     = document.getElementById('results');
var compareResults  = document.getElementById('compare-results');
var sidebar         = document.getElementById('sidebar');
var sidebarOverlay  = document.getElementById('sidebar-overlay');
var historyList     = document.getElementById('history-list');
var progressLoader  = document.getElementById('progress-loader');
var progressBarFill = document.getElementById('progress-bar-fill');
var progressText    = document.getElementById('progress-text');

// ── State ──

var currentData = null;
var lastUrl = '';
var currentMode = 'url';
var _completedPersonas = 0;

// ── Helpers ──

function show(el) { el.classList.add('active'); }
function hide(el) { el.classList.remove('active'); }

function escapeHtml(str) {
  var d = document.createElement('div');
  d.textContent = str;
  return d.innerHTML;
}

function generateId() {
  return Math.random().toString(36).substring(2, 10);
}

function getDomain(url) {
  try { return new URL(url).hostname; } catch { return url; }
}

function safeHref(url) {
  if (!url) return '#';
  try {
    var u = new URL(url);
    if (u.protocol === 'http:' || u.protocol === 'https:') return url;
  } catch (e) {}
  return '#';
}

function formatTime(ts) {
  var d = new Date(ts);
  var now = new Date();
  var diff = now - d;
  if (diff < 60000) return 'just now';
  if (diff < 3600000) return Math.floor(diff / 60000) + 'm ago';
  if (diff < 86400000) return Math.floor(diff / 3600000) + 'h ago';
  return d.toLocaleDateString();
}

function clamp(v, lo, hi) {
  return Math.max(lo, Math.min(hi, v));
}

function getPreviewSentences(text, count) {
  count = count || 2;
  if (!text) return { preview: '', rest: '' };
  var re = /[.!?](?:\s|$)/g;
  var end = 0;
  var found = 0;
  var m;
  while ((m = re.exec(text)) !== null && found < count) {
    found++;
    end = m.index + m[0].length;
  }
  if (found === 0) return { preview: text, rest: '' };
  var preview = text.substring(0, end).trim();
  var rest = text.substring(end).trim();
  return { preview: preview, rest: rest };
}

// ── Normalise API Response ──

function normaliseResponse(data) {
  if (data.personas && data.debiaser) {
    return {
      title: data.title || data.article_title || 'Untitled',
      source_url: data.source_url || '',
      personas: data.personas,
      debiaser: data.debiaser,
      _raw: data
    };
  }
  return { title: 'Untitled', source_url: '', personas: [], debiaser: {}, _raw: data };
}

function validateAnalysisResponse(data) {
  if (!data || typeof data !== 'object') return 'Invalid response: expected an object';
  if (!Array.isArray(data.personas) || !data.debiaser) return 'Invalid response: missing personas/debiaser';
  return null;
}

// ── Math Utilities ──

function weightedMean(items) {
  var n = items.reduce(function(a, b) { return a + b.score * b.weight; }, 0);
  var d = items.reduce(function(a, b) { return a + b.weight; }, 0) || 1;
  return n / d;
}

function stdDev(items) {
  var mean = weightedMean(items);
  var wSum = items.reduce(function(a, b) { return a + b.weight; }, 0) || 1;
  var variance = items.reduce(function(acc, it) { return acc + it.weight * Math.pow(it.score - mean, 2); }, 0) / wSum;
  return Math.sqrt(variance);
}

function clusterByAgreement(sorted, gap) {
  gap = gap || 0.9;
  var clusters = [];
  var current = [];
  for (var i = 0; i < sorted.length; i++) {
    if (i === 0) { current.push(sorted[i]); }
    else {
      if (Math.abs(sorted[i].stance_score - sorted[i - 1].stance_score) > gap) {
        clusters.push(current); current = [sorted[i]];
      } else { current.push(sorted[i]); }
    }
  }
  if (current.length) clusters.push(current);
  return clusters;
}

function colourForAxes(economic, social) {
  var e = clamp(economic, -3, 3);
  var s = clamp(social, -3, 3);
  var h = 180 + ((e + 3) / 6) * 180;
  var l = 55 + ((s + 3) / 6) * 15;
  return 'hsl(' + h.toFixed(0) + ', 80%, ' + l.toFixed(0) + '%)';
}

// ── Error Handling ──

var ERROR_HINTS = {
  'Invalid URL': 'Check the URL format — it should start with http:// or https://.',
  'Empty article content': 'The page was fetched but no readable article text was found.',
  'Failed to fetch article': 'The URL could not be reached. Check that it is correct and publicly accessible.',
  'Article fetch timed out': 'The remote site took too long to respond. Try again or use a different source.',
  'Page not found (404)': 'The URL returned a 404. The article may have been removed.',
  'Article is behind a paywall': 'Try the "Text" tab to paste the content directly.',
  'All LLM providers failed': 'Add an API key in Settings (⚙ icon) or check your existing keys.',
  'No house keys configured': 'Add your own API key in Settings (⚙ icon) to continue.',
};

function classifyError(err) {
  if (err._parsed) return err._parsed;
  if (err instanceof TypeError && err.message === 'Failed to fetch') {
    return { title: 'Cannot reach scraper', body: 'The Cloudflare Worker is unreachable.', hint: 'Check that WORKER_URL is set correctly in index.html.' };
  }
  // Match known error patterns
  var msg = err.message || String(err);
  for (var key in ERROR_HINTS) {
    if (msg.includes(key)) return { title: key, body: msg, hint: ERROR_HINTS[key] };
  }
  return { title: 'Something went wrong', body: msg, hint: null };
}

function showError(errObj) {
  document.getElementById('error-title').textContent = errObj.title;
  document.getElementById('error-body').textContent = errObj.body;
  var hintEl = document.getElementById('error-hint');
  if (errObj.hint) { hintEl.textContent = errObj.hint; hintEl.style.display = ''; }
  else { hintEl.style.display = 'none'; }
  show(errorBanner);
}

function hideError() { hide(errorBanner); }

// ── Skeleton Loading ──

var PERSONA_NAMES = [
  'Progressive Activist', 'Liberal Social Democrat', 'Centrist Technocrat',
  'Libertarian, Civil Liberties', 'Conservative, Fiscal', 'National Security Hawk',
  'Environmentalist Green', 'Populist, Anti-elite'
];

var PERSONA_ICONS = {
  'progressive_activist': '\u270A',
  'liberal_social_democrat': '\u2696',
  'centrist_technocrat': '\u2699',
  'libertarian_civil': '\uD83D\uDD13',
  'conservative_fiscal': '\uD83D\uDCB0',
  'national_security_hawk': '\uD83E\uDD85',
  'environmentalist_green': '\uD83C\uDF3F',
  'populist_anti_elite': '\uD83D\uDCE2',
};

// Track skeleton cards by persona ID for live updates
var _skeletonCards = {};

function buildSkeletonCard(personaId, personaName) {
  var card = document.createElement('div');
  card.className = 'persona-card loading';
  card.dataset.personaId = personaId;
  card.innerHTML =
    '<div class="persona-card-inner">' +
      '<div class="persona-card-spinner"></div>' +
      '<span class="persona-card-loading-label">' + escapeHtml(personaName || 'Loading...') + '</span>' +
    '</div>';
  return card;
}

function showSkeletons() {
  skeletonGrid.innerHTML = '';
  _skeletonCards = {};
  var ids = ['progressive_activist','liberal_social_democrat','centrist_technocrat',
             'libertarian_civil','conservative_fiscal','national_security_hawk',
             'environmentalist_green','populist_anti_elite'];
  ids.forEach(function(id, i) {
    var card = buildSkeletonCard(id, PERSONA_NAMES[i]);
    _skeletonCards[id] = card;
    skeletonGrid.appendChild(card);
  });
  show(skeletonGrid);
}

function hideSkeletons() { hide(skeletonGrid); }

// Replace a skeleton card with a real persona card as results arrive
function liveUpdatePersonaCard(personaOutput) {
  var skeleton = _skeletonCards[personaOutput.id];
  if (!skeleton) return;
  var realCard = buildPersonaCard(personaOutput);
  realCard.classList.add('persona-card-live-in');
  skeleton.parentNode.replaceChild(realCard, skeleton);
  _skeletonCards[personaOutput.id] = null;
  // Animate in
  requestAnimationFrame(function() { realCard.classList.remove('persona-card-live-in'); });
}

// ── Progress Loader ──

var progressInterval = null;
var progressValue = 0;

function showProgress(message) {
  progressValue = 0;
  _completedPersonas = 0;
  progressBarFill.style.width = '0%';
  progressText.textContent = message || 'Analysing article\u2026';
  show(progressLoader);
  clearInterval(progressInterval);
  progressInterval = setInterval(function() {
    // Progress is driven by actual persona completions (each = ~10%)
    var target = 5 + (_completedPersonas / 8) * 75;
    if (progressValue < target) {
      progressValue = Math.min(progressValue + 2, target);
      progressBarFill.style.width = progressValue + '%';
    }
    if (progressValue < 8) {
      progressText.textContent = 'Scraping article content...';
    } else if (_completedPersonas < 8) {
      progressText.textContent = 'Analysing: ' + _completedPersonas + '/8 personas complete...';
    } else if (progressValue < 95) {
      progressText.textContent = 'Synthesising debiased summary...';
    }
  }, 300);
}

function hideProgress() {
  clearInterval(progressInterval);
  progressBarFill.style.width = '100%';
  progressText.textContent = 'Complete!';
  setTimeout(function() {
    hide(progressLoader);
    progressBarFill.style.width = '0%';
  }, 400);
}

function setLoading(on) {
  analyzeBtn.disabled = on;
  document.getElementById('text-analyze-btn').disabled = on;
  document.getElementById('compare-btn').disabled = on;
  if (on) {
    hideError();
    hide(resultsArea);
    hide(compareResults);
    showProgress();
    showSkeletons();
  } else {
    hideProgress();
    hideSkeletons();
  }
}

// ── Render: Spectrum Bar ──

function renderSpectrum(spectrumScore) {
  var val = clamp(spectrumScore, -3, 3);
  var pct = ((val + 3) / 6) * 100;
  requestAnimationFrame(function() {
    document.getElementById('spectrum-fill').style.width = pct + '%';
    document.getElementById('spectrum-dot').style.left = pct + '%';
  });
  document.getElementById('spectrum-value').textContent = 'Value ' + val.toFixed(2) + ' on a \u22123 to +3 Liberty\u2013Order axis.';
}

// ── Render: Disagreement Meter ──

function renderDisagreementMeter(personas) {
  var scores = personas.map(function(p) { return { score: p.stance_score, weight: p.confidence || 1 }; });
  var stdev = scores.length > 1 ? stdDev(scores) : 0;
  var level = stdev < 0.5 ? 'low' : stdev < 1.2 ? 'medium' : 'high';
  var pct = Math.min(100, Math.round((stdev / 2) * 100));
  var meterFill = document.getElementById('meter-fill');
  meterFill.className = 'meter-fill ' + level;
  requestAnimationFrame(function() { meterFill.style.width = pct + '%'; });
  document.getElementById('meter-stdev').textContent = 'Std dev ' + stdev.toFixed(2);
  var sorted = personas.slice().sort(function(a, b) { return a.stance_score - b.stance_score; });
  var clusters = clusterByAgreement(sorted);
  document.getElementById('meter-clusters').textContent = clusters.length + ' cluster' + (clusters.length !== 1 ? 's' : '') + ' detected: ' +
    clusters.map(function(c) { return c.length + ' persona' + (c.length !== 1 ? 's' : ''); }).join(', ');
  return { sorted: sorted, clusters: clusters };
}

// ── Render: 2D Axis Grid ──

function renderAxisGrid(personas) {
  var grid = document.getElementById('axis-grid');
  grid.querySelectorAll('.axis-dot, .axis-unavailable, .gridline-h, .gridline-v, .axis-center-h, .axis-center-v').forEach(function(d) { d.remove(); });
  var withAxes = personas.filter(function(p) { return p.axes; });
  if (withAxes.length === 0) {
    var msg = document.createElement('div');
    msg.className = 'axis-unavailable';
    msg.textContent = 'Axis data unavailable for this analysis';
    grid.appendChild(msg);
    return;
  }
  [0, 25, 50, 75, 100].forEach(function(p) {
    var h = document.createElement('div'); h.className = 'gridline-h'; h.style.top = p + '%'; grid.appendChild(h);
    var v = document.createElement('div'); v.className = 'gridline-v'; v.style.left = p + '%'; grid.appendChild(v);
  });
  var ch = document.createElement('div'); ch.className = 'axis-center-h'; grid.appendChild(ch);
  var cv = document.createElement('div'); cv.className = 'axis-center-v'; grid.appendChild(cv);
  var toPct = function(v) { return ((clamp(v, -3, 3) + 3) / 6) * 100; };
  withAxes.forEach(function(p) {
    var dot = document.createElement('div');
    dot.className = 'axis-dot';
    dot.style.left = 'calc(' + toPct(p.axes.economic) + '% - 7px)';
    dot.style.top = 'calc(' + (100 - toPct(p.axes.social)) + '% - 7px)';
    dot.style.background = colourForAxes(p.axes.economic, p.axes.social);
    dot.title = p.title + ': econ ' + p.axes.economic.toFixed(1) + ', social ' + p.axes.social.toFixed(1);
    grid.appendChild(dot);
  });
  if (withAxes.length < personas.length) {
    var notice = document.createElement('div');
    notice.className = 'axis-unavailable axis-partial';
    notice.textContent = withAxes.length + ' of ' + personas.length + ' personas provided axis data';
    grid.appendChild(notice);
  }
  document.getElementById('legend-dot-lo').style.background = colourForAxes(-3, -3);
  document.getElementById('legend-dot-hi').style.background = colourForAxes(3, 3);
}

// ── Render: Persona Card ──

function buildPersonaCard(p) {
  var card = document.createElement('div');
  card.className = 'persona-card';
  var conf = Math.round((p.confidence || 0) * 100);
  var stanceText = (p.stance_score >= 0 ? '+' : '') + p.stance_score.toFixed(1);
  var icon = PERSONA_ICONS[p.id] || '\uD83D\uDC64';
  var parts = getPreviewSentences(p.summary || '', 2);
  var hasExpandContent = parts.rest.length > 0 ||
    (p.key_claims && p.key_claims.length > 0) ||
    (p.fact_checks && p.fact_checks.length > 0) ||
    (p.caveats && p.caveats.length > 0);

  if (hasExpandContent) {
    card.setAttribute('aria-expanded', 'false');
    card.setAttribute('tabindex', '0');
    card.setAttribute('role', 'button');
  }

  var html = '<div class="persona-card-header">' +
    '<div class="persona-title-row">' +
      '<span class="persona-icon">' + icon + '</span>' +
      '<h4 class="persona-title">' + escapeHtml(p.title) + '</h4>' +
    '</div>' +
    '<div class="persona-header-right">' +
      '<span class="persona-badge">Score ' + stanceText + ' &middot; Conf ' + conf + '%</span>' +
      (hasExpandContent ? '<span class="persona-chevron" aria-hidden="true">&#9662;</span>' : '') +
    '</div>' +
  '</div>' +
  '<p class="persona-preview">' + escapeHtml(parts.preview) + '</p>';

  if (hasExpandContent) {
    html += '<div class="persona-details"><div class="persona-details-inner">';
    if (parts.rest) html += '<p class="persona-summary">' + escapeHtml(parts.rest) + '</p>';
    if (p.key_claims && p.key_claims.length > 0) {
      html += '<div class="persona-section-title">Key claims</div><ul class="persona-claims">';
      p.key_claims.forEach(function(c) { html += '<li>' + escapeHtml(c) + '</li>'; });
      html += '</ul>';
    }
    if (p.fact_checks && p.fact_checks.length > 0) {
      html += '<div class="persona-section-title">Fact checks</div><ul class="fact-check-list">';
      p.fact_checks.forEach(function(fc) {
        html += '<li class="fact-check-item">' +
          '<div class="fact-check-claim">' + escapeHtml(fc.claim) + '</div>' +
          '<div class="fact-check-detail">' +
            '<span class="assessment-badge ' + escapeHtml(fc.assessment) + '">' + escapeHtml(fc.assessment) + '</span> &middot; ' +
            escapeHtml(fc.rationale) +
          '</div></li>';
      });
      html += '</ul>';
    }
    if (p.caveats && p.caveats.length > 0) {
      html += '<div class="persona-section-title">Caveats</div><ul class="persona-caveats">';
      p.caveats.forEach(function(c) { html += '<li>' + escapeHtml(c) + '</li>'; });
      html += '</ul>';
    }
    html += '</div></div>';
  }

  card.innerHTML = html;

  if (hasExpandContent) {
    var toggleCard = function() {
      var expanded = card.getAttribute('aria-expanded') === 'true';
      card.setAttribute('aria-expanded', expanded ? 'false' : 'true');
    };
    card.addEventListener('click', function(e) { if (e.target.tagName !== 'A') toggleCard(); });
    card.addEventListener('keydown', function(e) {
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleCard(); }
    });
  }
  return card;
}

// ── Render: Source Credibility ──

function renderSourceCredibility(rawData) {
  var section = document.getElementById('source-credibility');
  var meta = rawData.source_meta;
  if (!meta) { section.classList.remove('active'); return; }
  document.getElementById('source-publication').textContent = meta.publication || 'Unknown';
  var biasEl = document.getElementById('source-bias');
  var bias = meta.known_bias || 'unknown';
  biasEl.textContent = bias;
  biasEl.className = 'source-cred-badge';
  var biasLower = bias.toLowerCase();
  if (biasLower.includes('left') && !biasLower.includes('center')) biasEl.classList.add('left');
  else if (biasLower.includes('right') && !biasLower.includes('center')) biasEl.classList.add('right');
  else if (biasLower.includes('center') || biasLower.includes('centre')) biasEl.classList.add('center');
  else biasEl.classList.add('unknown');
  document.getElementById('source-ownership').textContent = meta.ownership_type || 'Unknown';
  section.classList.add('active');
}

// ── Render: Tone & Framing ──

function renderToneAnalysis(rawData) {
  var section = document.getElementById('tone-section');
  var tone = rawData.tone_analysis;
  if (!tone) { section.classList.remove('active'); return; }
  var pct = Math.round(clamp(tone.objectivity_score || 0, 0, 1) * 100);
  document.getElementById('tone-obj-fill').style.width = pct + '%';
  document.getElementById('tone-obj-value').textContent = pct + '%';
  document.getElementById('tone-emotional').textContent = tone.emotional_tone || 'Unknown';
  document.getElementById('tone-framing').textContent = tone.framing_strategy || 'Not identified';
  var devicesList = document.getElementById('tone-devices-list');
  devicesList.innerHTML = '';
  (tone.rhetorical_devices || []).forEach(function(d) {
    var tag = document.createElement('span'); tag.className = 'tone-device-tag'; tag.textContent = d; devicesList.appendChild(tag);
  });
  if (!tone.rhetorical_devices || tone.rhetorical_devices.length === 0) {
    var none = document.createElement('span'); none.className = 'tone-device-tag'; none.textContent = 'None detected'; devicesList.appendChild(none);
  }
  show(section);
}

// ── Render: Full Results ──

function renderResults(rawData, sourceUrl) {
  var validationError = validateAnalysisResponse(rawData);
  if (validationError) {
    showError({ title: 'Invalid analysis data', body: validationError, hint: 'Try running the analysis again.' });
    return;
  }
  var data = normaliseResponse(rawData);
  currentData = data;

  document.getElementById('article-title').textContent = data.title;
  var sourceLink = document.getElementById('article-source');
  var url = sourceUrl || data.source_url;
  if (url && url.startsWith('http')) {
    sourceLink.textContent = url; sourceLink.href = safeHref(url); sourceLink.style.display = '';
  } else { sourceLink.style.display = 'none'; }

  renderSourceCredibility(rawData);
  document.getElementById('article-summary-text').textContent = data.debiaser.truth_seeking_summary || '';
  renderToneAnalysis(rawData);
  renderSpectrum(data.debiaser.spectrum_score || 0);
  var clusterInfo = renderDisagreementMeter(data.personas);

  var show2dCheckbox = document.getElementById('show-2d');
  renderAxisGrid(data.personas);
  if (show2dCheckbox.checked) show(document.getElementById('axis-grid-section'));

  var partialNotice = document.getElementById('partial-notice');
  var warnings = rawData.warnings || [];
  if (warnings.length > 0) {
    partialNotice.textContent = warnings.join(' \u2022 '); show(partialNotice);
  } else if (data.personas.length < 8) {
    var failed = 8 - data.personas.length;
    partialNotice.textContent = 'Partial results: ' + data.personas.length + '/8 personas responded. ' + failed + ' failed.';
    show(partialNotice);
  } else { hide(partialNotice); }

  var clustersContainer = document.getElementById('persona-clusters');
  clustersContainer.innerHTML = '';
  if (clusterInfo.clusters.length > 1) {
    var clusterSummary = document.createElement('div');
    clusterSummary.className = 'cluster-meta';
    clusterSummary.textContent = clusterInfo.clusters.length + ' opinion clusters detected \u00B7 personas sorted by agreement level';
    clustersContainer.appendChild(clusterSummary);
  }

  var sortedPersonas = data.personas.slice().sort(function(a, b) { return Math.abs(a.stance_score) - Math.abs(b.stance_score); });
  var grid = document.createElement('div');
  grid.className = 'cluster-cards';
  sortedPersonas.forEach(function(p) { grid.appendChild(buildPersonaCard(p)); });
  clustersContainer.appendChild(grid);

  document.getElementById('debiaser-summary').textContent = data.debiaser.truth_seeking_summary || '';

  function fillList(elId, items) {
    var el = document.getElementById(elId);
    el.innerHTML = '';
    (items || []).forEach(function(item) {
      var li = document.createElement('li'); li.textContent = item; el.appendChild(li);
    });
  }
  fillList('debiaser-consensus', data.debiaser.consensus_points);
  fillList('debiaser-disagreements', data.debiaser.disagreements);
  fillList('debiaser-bias', data.debiaser.likely_bias_drivers);

  show(resultsArea);
}

// ── Input Tabs ──

document.querySelectorAll('.input-tab').forEach(function(tab) {
  tab.addEventListener('click', function() {
    document.querySelectorAll('.input-tab').forEach(function(t) { t.classList.remove('active'); });
    tab.classList.add('active');
    currentMode = tab.dataset.tab;
    form.classList.toggle('hidden', currentMode !== 'url');
    document.getElementById('text-input-area').classList.toggle('active', currentMode === 'text');
    document.getElementById('compare-input-area').classList.toggle('active', currentMode === 'compare');
    hide(resultsArea); hide(compareResults); hideError();
  });
});

document.getElementById('show-2d').addEventListener('change', function() {
  var section = document.getElementById('axis-grid-section');
  if (this.checked && currentData) show(section); else hide(section);
});

// ── Copy to Clipboard ──

function copyToClipboard(text, btn, labelEl) {
  navigator.clipboard.writeText(text).then(function() {
    btn.classList.add('copied');
    if (labelEl) labelEl.textContent = 'Copied';
    setTimeout(function() { btn.classList.remove('copied'); if (labelEl) labelEl.textContent = 'Copy'; }, 2000);
  }).catch(function() {
    var ta = document.createElement('textarea');
    ta.value = text; ta.style.position = 'fixed'; ta.style.opacity = '0';
    document.body.appendChild(ta); ta.select(); document.execCommand('copy'); document.body.removeChild(ta);
    btn.classList.add('copied');
    if (labelEl) labelEl.textContent = 'Copied';
    setTimeout(function() { btn.classList.remove('copied'); if (labelEl) labelEl.textContent = 'Copy'; }, 2000);
  });
}

// ── History (localStorage) ──

var HISTORY_KEY = 'politicaldebaiser_history';
var MAX_HISTORY = 50;

function getHistory() { try { return JSON.parse(localStorage.getItem(HISTORY_KEY)) || []; } catch { return []; } }
function saveHistory(h) { localStorage.setItem(HISTORY_KEY, JSON.stringify(h)); }

function addToHistory(title, url, rawData) {
  var history = getHistory();
  var entry = { id: generateId(), title: title, url: url || '', timestamp: Date.now(), data: rawData };
  history.unshift(entry);
  if (history.length > MAX_HISTORY) history = history.slice(0, MAX_HISTORY);
  saveHistory(history);
  renderHistory();
  return entry.id;
}

function deleteFromHistory(id) { saveHistory(getHistory().filter(function(h) { return h.id !== id; })); renderHistory(); }
function clearHistory() { localStorage.removeItem(HISTORY_KEY); renderHistory(); }

function renderHistory() {
  var history = getHistory();
  if (history.length === 0) { historyList.innerHTML = '<div class="history-empty">No analysis history yet.</div>'; return; }
  historyList.innerHTML = '';
  history.forEach(function(item) {
    var el = document.createElement('div');
    el.className = 'history-item';
    el.innerHTML =
      '<div class="history-item-content">' +
        '<div class="history-item-title">' + escapeHtml(item.title || 'Untitled') + '</div>' +
        '<div class="history-item-domain">' + escapeHtml(item.url ? getDomain(item.url) : 'Text input') + '</div>' +
        '<div class="history-item-time">' + formatTime(item.timestamp) + '</div>' +
      '</div>' +
      '<button class="history-item-delete" title="Delete">&times;</button>';
    el.querySelector('.history-item-content').addEventListener('click', function() {
      var check = validateAnalysisResponse(item.data);
      if (check) { showError({ title: 'Corrupted entry', body: check, hint: 'Delete and re-analyse.' }); return; }
      renderResults(item.data, item.url);
      closeSidebar();
    });
    el.querySelector('.history-item-delete').addEventListener('click', function(e) { e.stopPropagation(); deleteFromHistory(item.id); });
    historyList.appendChild(el);
  });
}

// ── Sidebar ──

function openSidebar() { sidebar.classList.add('open'); sidebarOverlay.classList.add('open'); renderHistory(); }
function closeSidebar() { sidebar.classList.remove('open'); sidebarOverlay.classList.remove('open'); }

document.getElementById('sidebar-toggle').addEventListener('click', openSidebar);
document.getElementById('sidebar-close').addEventListener('click', closeSidebar);
sidebarOverlay.addEventListener('click', closeSidebar);
document.getElementById('clear-history').addEventListener('click', clearHistory);

// ── Export ──

function downloadFile(content, filename, type) {
  var blob = new Blob([content], { type: type });
  var u = URL.createObjectURL(blob);
  var a = document.createElement('a');
  a.href = u; a.download = filename; document.body.appendChild(a); a.click(); document.body.removeChild(a); URL.revokeObjectURL(u);
}

document.getElementById('export-json').addEventListener('click', function() {
  if (!currentData) return;
  var json = JSON.stringify(currentData._raw || currentData, null, 2);
  downloadFile(json, (currentData.title || 'analysis').replace(/[^a-z0-9]/gi, '_').slice(0, 40) + '.json', 'application/json');
});

document.getElementById('export-text').addEventListener('click', function() {
  if (!currentData) return;
  var lines = ['Debiaser Analysis Report', '='.repeat(40), '', 'Article: ' + currentData.title];
  if (lastUrl) lines.push('Source: ' + lastUrl);
  lines.push('', 'Spectrum Score: ' + (currentData.debiaser.spectrum_score || 0).toFixed(2), '');
  currentData.personas.forEach(function(p) {
    lines.push('-'.repeat(40), p.title + ' (Stance: ' + (p.stance_score >= 0 ? '+' : '') + p.stance_score.toFixed(1) + ', Confidence: ' + Math.round((p.confidence || 0) * 100) + '%)', '', p.summary, '');
    if (p.key_claims && p.key_claims.length) { lines.push('Key Claims:'); p.key_claims.forEach(function(c) { lines.push('  - ' + c); }); lines.push(''); }
    if (p.fact_checks && p.fact_checks.length) { lines.push('Fact Checks:'); p.fact_checks.forEach(function(fc) { lines.push('  [' + fc.assessment + '] ' + fc.claim + ' \u2014 ' + fc.rationale); }); lines.push(''); }
  });
  downloadFile(lines.join('\n'), (currentData.title || 'analysis').replace(/[^a-z0-9]/gi, '_').slice(0, 40) + '.txt', 'text/plain');
});

document.getElementById('share-link').addEventListener('click', function() {
  if (!currentData) return;
  var id = addToHistory(currentData.title, lastUrl, currentData._raw || currentData);
  var shareUrl = window.location.origin + window.location.pathname + '#/history/' + id;
  var btn = document.getElementById('share-link');
  copyToClipboard(shareUrl, btn, null);
  btn.textContent = 'Link Copied!';
  setTimeout(function() { btn.textContent = 'Share Link'; }, 2000);
});

// ── Core Analysis Flow ──

async function doAnalyze(url) {
  setLoading(true);
  lastUrl = url;
  try {
    // Step 1: Scrape article via Worker
    progressText.textContent = 'Scraping article...';
    var article = await window.DebAIser.scrapeArticle(url);

    // Step 2: Run analysis with live persona updates
    var result = await window.DebAIser.analyzeContent(
      article.body,
      article.title,
      article.source_url,
      {
        onPersonaComplete: function(personaOutput) {
          _completedPersonas++;
          liveUpdatePersonaCard(personaOutput);
        },
        onProgress: function(msg) { progressText.textContent = msg; },
      }
    );

    setLoading(false);
    renderResults(result, url);
    addToHistory(result.title || 'Untitled', url, result);
  } catch (err) {
    setLoading(false);
    showError(classifyError(err));
  }
}

form.addEventListener('submit', function(e) {
  e.preventDefault();
  var url = urlInput.value.trim();
  if (!url) return;
  doAnalyze(url);
});

// ── Text Analysis ──

document.getElementById('text-analyze-btn').addEventListener('click', async function() {
  var text = document.getElementById('text-content-input').value.trim();
  if (!text) return;
  var title = document.getElementById('text-title-input').value.trim() || 'Untitled Text';
  setLoading(true); lastUrl = '';
  try {
    var result = await window.DebAIser.analyzeContent(text, title, null, {
      onPersonaComplete: function(p) { _completedPersonas++; liveUpdatePersonaCard(p); },
      onProgress: function(msg) { progressText.textContent = msg; },
    });
    setLoading(false);
    renderResults(result, '');
    addToHistory(result.title || title, '', result);
  } catch (err) {
    setLoading(false);
    showError(classifyError(err));
  }
});

// ── Compare ──

document.getElementById('compare-btn').addEventListener('click', async function() {
  var urlA = document.getElementById('compare-url-a').value.trim();
  var urlB = document.getElementById('compare-url-b').value.trim();
  if (!urlA || !urlB) return;
  setLoading(true); hide(compareResults);
  try {
    var [artA, artB] = await Promise.all([
      window.DebAIser.scrapeArticle(urlA),
      window.DebAIser.scrapeArticle(urlB),
    ]);
    var [resA, resB] = await Promise.all([
      window.DebAIser.analyzeContent(artA.body, artA.title, artA.source_url),
      window.DebAIser.analyzeContent(artB.body, artB.title, artB.source_url),
    ]);
    setLoading(false);
    renderCompareResults(resA, urlA, resB, urlB);
    addToHistory(resA.title || 'Article A', urlA, resA);
    addToHistory(resB.title || 'Article B', urlB, resB);
  } catch (err) {
    setLoading(false);
    showError(classifyError(err));
  }
});

function renderCompareResults(rawA, urlA, rawB, urlB) {
  var dataA = normaliseResponse(rawA);
  var dataB = normaliseResponse(rawB);
  var colA = document.getElementById('compare-col-a');
  var colB = document.getElementById('compare-col-b');
  colA.innerHTML = '<div class="compare-col-header">' + escapeHtml(dataA.title) + '</div>' +
    '<div class="article-info-card" style="margin-bottom:1rem">' +
      '<a class="article-source" href="' + escapeHtml(safeHref(urlA)) + '" target="_blank" rel="noopener noreferrer">' + escapeHtml(getDomain(urlA)) + '</a>' +
      '<p class="article-summary-text">Spectrum: ' + (dataA.debiaser.spectrum_score || 0).toFixed(2) + '</p>' +
    '</div>';
  dataA.personas.forEach(function(p) { colA.appendChild(buildPersonaCard(p)); });
  colB.innerHTML = '<div class="compare-col-header">' + escapeHtml(dataB.title) + '</div>' +
    '<div class="article-info-card" style="margin-bottom:1rem">' +
      '<a class="article-source" href="' + escapeHtml(safeHref(urlB)) + '" target="_blank" rel="noopener noreferrer">' + escapeHtml(getDomain(urlB)) + '</a>' +
      '<p class="article-summary-text">Spectrum: ' + (dataB.debiaser.spectrum_score || 0).toFixed(2) + '</p>' +
    '</div>';
  dataB.personas.forEach(function(p) { colB.appendChild(buildPersonaCard(p)); });
  hide(resultsArea); show(compareResults);
}

// ── Retry ──

document.getElementById('retry-btn').addEventListener('click', function() {
  hideError();
  if (currentMode === 'url') { var url = lastUrl || urlInput.value.trim(); if (url) doAnalyze(url); }
  else if (currentMode === 'text') { document.getElementById('text-analyze-btn').click(); }
  else if (currentMode === 'compare') { document.getElementById('compare-btn').click(); }
});

// ── Hash Routing ──

function handleHashRoute() {
  var hash = window.location.hash;
  if (!hash.startsWith('#/history/')) return;
  var id = hash.replace('#/history/', '');
  var entry = getHistory().find(function(h) { return h.id === id; });
  if (entry) renderResults(entry.data, entry.url);
}
window.addEventListener('hashchange', handleHashRoute);
handleHashRoute();

// ── Health Indicator (Worker-based) ──

var healthDot = document.getElementById('health-dot');
var healthLabel = document.getElementById('health-label');

function checkHealth() {
  var workerUrl = window.WORKER_URL || '';
  if (!workerUrl || workerUrl.includes('YOUR_SUBDOMAIN')) {
    healthDot.className = 'health-dot error';
    healthLabel.textContent = 'Worker not configured';
    healthDot.parentElement.title = 'Set WORKER_URL in index.html';
    return;
  }
  fetch(workerUrl + '/health').then(function(res) {
    if (res.ok) {
      healthDot.className = 'health-dot ok';
      healthLabel.textContent = 'Ready';
      healthDot.parentElement.title = 'Worker is reachable';
    } else { setHealthError(); }
  }).catch(setHealthError);
}

function setHealthError() {
  healthDot.className = 'health-dot error';
  healthLabel.textContent = 'Worker offline';
  healthDot.parentElement.title = 'Cloudflare Worker is unreachable';
}

checkHealth();
setInterval(checkHealth, 30000);

// ── Settings Modal ──

var SETTINGS_KEY = 'politicaldebaiser_api_keys';
var settingsModal = document.getElementById('settings-modal');
var settingsOverlay = document.getElementById('settings-overlay');

function getStoredKeys() { try { return JSON.parse(localStorage.getItem(SETTINGS_KEY)) || {}; } catch { return {}; } }
function saveStoredKeys(k) { localStorage.setItem(SETTINGS_KEY, JSON.stringify(k)); }

function openSettings() {
  var stored = getStoredKeys();
  document.getElementById('setting-groq-key').value = stored.groq_api_key || '';
  document.getElementById('setting-gemini-key').value = stored.gemini_api_key || '';
  document.getElementById('setting-hf-key').value = stored.hf_api_key || '';
  refreshConfigStatus();
  settingsModal.classList.add('active');
  settingsOverlay.classList.add('active');
}

function closeSettings() { settingsModal.classList.remove('active'); settingsOverlay.classList.remove('active'); }

function refreshConfigStatus() {
  var stored = getStoredKeys();
  var keys = { groq: 'groq_api_key', gemini: 'gemini_api_key', hf: 'hf_api_key' };
  var ids = { groq: 'groq-status', gemini: 'gemini-status', hf: 'hf-status' };
  Object.keys(keys).forEach(function(provider) {
    var el = document.getElementById(ids[provider]);
    if (!el) return;
    var hasKey = !!(stored[keys[provider]]);
    el.textContent = hasKey ? 'Key saved' : 'Using house key';
    el.className = 'settings-status' + (hasKey ? ' configured' : '');
  });
}

document.getElementById('settings-toggle').addEventListener('click', openSettings);
document.getElementById('settings-close').addEventListener('click', closeSettings);
settingsOverlay.addEventListener('click', closeSettings);

document.getElementById('settings-save').addEventListener('click', function() {
  var keys = {
    groq_api_key: document.getElementById('setting-groq-key').value.trim(),
    gemini_api_key: document.getElementById('setting-gemini-key').value.trim(),
    hf_api_key: document.getElementById('setting-hf-key').value.trim(),
  };
  saveStoredKeys(keys);
  refreshConfigStatus();
  closeSettings();
});

document.getElementById('settings-clear').addEventListener('click', function() {
  ['setting-groq-key','setting-gemini-key','setting-hf-key'].forEach(function(id) {
    document.getElementById(id).value = '';
  });
  saveStoredKeys({});
  refreshConfigStatus();
});

document.querySelectorAll('.settings-toggle-vis').forEach(function(btn) {
  btn.addEventListener('click', function() {
    var input = document.getElementById(btn.getAttribute('data-target'));
    input.type = input.type === 'password' ? 'text' : 'password';
  });
});

// ── Init ──
renderHistory();
