// Debiaser v3 — Client-side Application
// Supports both new AnalysisResult shape (personas + debiaser) and legacy shape (analyses + synthesis)

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
var currentMode = 'url'; // 'url' | 'text' | 'compare'

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

// ── Normalise API Response ──
// Converts either new (personas+debiaser) or legacy (analyses+synthesis) shape
// into a unified internal format.

function normaliseResponse(data) {
  // Already v3 shape?
  if (data.personas && data.debiaser) {
    return {
      title: data.title || data.article_title || 'Untitled',
      source_url: data.source_url || '',
      personas: data.personas,
      debiaser: data.debiaser,
      _raw: data
    };
  }

  // Legacy v2 shape: convert archetypes to pseudo-personas
  var personas = (data.analyses || []).map(function(a) {
    // Map alignment_score (0..1) to stance_score (-3..+3) roughly
    var stance = (a.alignment_score - 0.5) * 6;
    return {
      id: a.archetype || 'unknown',
      title: a.archetype ? (a.archetype.charAt(0).toUpperCase() + a.archetype.slice(1)) : 'Unknown',
      stance_score: Number(stance.toFixed(1)),
      confidence: a.alignment_score || 0.5,
      summary: a.summary || '',
      key_claims: a.highlights || [],
      fact_checks: [],
      caveats: []
    };
  });

  // Build debiaser from synthesis/commonalities
  var debiaser = {
    truth_seeking_summary: data.synthesis || data.article_summary || '',
    consensus_points: data.commonalities || [],
    disagreements: [],
    likely_bias_drivers: [],
    spectrum_score: 0,
    spectrum_explain: 'Derived from legacy archetype alignment scores.'
  };

  // Compute spectrum_score as weighted mean of persona stances
  if (personas.length > 0) {
    var wMean = weightedMean(personas.map(function(p) { return { score: p.stance_score, weight: p.confidence }; }));
    debiaser.spectrum_score = Number(wMean.toFixed(2));
  }

  return {
    title: data.article_title || data.title || 'Untitled',
    source_url: data.source_url || '',
    personas: personas,
    debiaser: debiaser,
    _raw: data
  };
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
    if (i === 0) {
      current.push(sorted[i]);
    } else {
      if (Math.abs(sorted[i].stance_score - sorted[i - 1].stance_score) > gap) {
        clusters.push(current);
        current = [sorted[i]];
      } else {
        current.push(sorted[i]);
      }
    }
  }
  if (current.length) clusters.push(current);
  return clusters;
}

function colourForAxes(economic, social) {
  var e = clamp(economic, -3, 3);
  var s = clamp(social, -3, 3);
  var h = 220 + ((e + 3) / 6) * 120;
  var l = 60 - ((s + 3) / 6) * 25;
  return 'hsl(' + h.toFixed(0) + ', 70%, ' + l.toFixed(0) + '%)';
}

// ── Error Handling ──

var ERROR_HINTS = {
  'Ollama is unavailable': 'Make sure Ollama is running: run "ollama serve" in a terminal.',
  'Ollama request timed out': 'The model may be loading for the first time. Try again in a moment.',
  'Invalid URL': 'Check the URL format \u2014 it should start with http:// or https://.',
  'Empty article content': 'The page was fetched but no readable article text was found. Try a different URL.',
  'Failed to fetch article': 'The URL could not be reached. Check that it is correct and publicly accessible.',
  'Article fetch timed out': 'The remote site took too long to respond. Try again or use a different source.',
  'Page not found': 'The URL returned a 404. The article may have been removed or the URL is incorrect.',
  'Article behind paywall': 'This article is behind a paywall. Try a different source or a non-paywalled link.',
  'Not an HTML page': 'The URL points to a non-HTML resource (e.g., PDF, image). Paste a link to an article page.',
  'Analysis failed': 'The AI analysis encountered an error. Try again in a moment.',
  'No analyses provided': 'No analysis data was available to synthesize. Run an analysis first.'
};

function classifyError(err) {
  if (err._parsed) return err._parsed;
  // Detect network-level failures (server unreachable)
  if (err instanceof TypeError && err.message === 'Failed to fetch') {
    return {
      title: 'Server unreachable',
      body: 'Could not connect to the Debiaser server.',
      hint: 'Check that the server is running and try again.'
    };
  }
  return { title: 'Something went wrong', body: err.message || String(err), hint: null };
}

function showError(errObj) {
  var title = errObj.title;
  var body = errObj.body;
  var hint = errObj.hint;
  document.getElementById('error-title').textContent = title;
  document.getElementById('error-body').textContent = body;
  var hintEl = document.getElementById('error-hint');
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
  var text = await res.text().catch(function() { return ''; });
  try {
    var json = JSON.parse(text);
    var title = json.error || 'Request failed';
    var body = json.details || 'Server responded with ' + res.status;
    var hint = ERROR_HINTS[json.error] || null;
    return { _parsed: { title: title, body: body, hint: hint } };
  } catch (e) {
    return { _parsed: { title: 'Request failed', body: text || 'Server responded with ' + res.status, hint: null } };
  }
}

// ── Skeleton Loading ──

function buildSkeletonCard(personaName) {
  var card = document.createElement('div');
  card.className = 'persona-card loading';
  card.innerHTML =
    '<div class="persona-card-inner">' +
      '<div class="persona-card-spinner"></div>' +
      '<span class="persona-card-loading-label">' + escapeHtml(personaName || 'Loading...') + '</span>' +
    '</div>';
  return card;
}

function showSkeletons() {
  skeletonGrid.innerHTML = '';
  for (var i = 0; i < 8; i++) {
    var name = PERSONA_NAMES[i] || 'Persona ' + (i + 1);
    skeletonGrid.appendChild(buildSkeletonCard(name));
  }
  show(skeletonGrid);
}

function hideSkeletons() {
  hide(skeletonGrid);
}

// ── Progress Loader ──

var PERSONA_NAMES = [
  'Progressive Activist',
  'Liberal Social Democrat',
  'Centrist Technocrat',
  'Libertarian, Civil Liberties',
  'Conservative, Fiscal',
  'National Security Hawk',
  'Environmentalist Green',
  'Populist, Anti-elite'
];

var progressInterval = null;
var progressValue = 0;
var progressPersonaIdx = 0;

function showProgress(message) {
  progressValue = 0;
  progressPersonaIdx = 0;
  progressBarFill.style.width = '0%';
  progressText.textContent = message || 'Analysing article...';
  show(progressLoader);

  // Simulate progress with persona names cycling
  clearInterval(progressInterval);
  progressInterval = setInterval(function() {
    if (progressValue < 85) {
      progressValue += Math.random() * 6 + 1.5;
      progressValue = Math.min(progressValue, 85);
      progressBarFill.style.width = progressValue + '%';
    }
    if (progressValue < 10) {
      progressText.textContent = 'Scraping article content...';
    } else if (progressValue < 75) {
      var name = PERSONA_NAMES[progressPersonaIdx % PERSONA_NAMES.length];
      progressText.textContent = 'Analysing as ' + name + '... (' + (progressPersonaIdx + 1) + '/8)';
      progressPersonaIdx++;
    } else if (progressValue < 82) {
      progressText.textContent = 'Synthesising debiased summary...';
    } else {
      progressText.textContent = 'Almost done...';
    }
  }, 800);
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

  var fill = document.getElementById('spectrum-fill');
  var dot = document.getElementById('spectrum-dot');
  var valText = document.getElementById('spectrum-value');

  requestAnimationFrame(function() {
    fill.style.width = pct + '%';
    dot.style.left = pct + '%';
  });

  valText.textContent = 'Value ' + val.toFixed(2) + ' on a \u22123 to +3 Liberty\u2013Order axis.';
}

// ── Render: Disagreement Meter ──

function renderDisagreementMeter(personas) {
  var scores = personas.map(function(p) { return { score: p.stance_score, weight: p.confidence || 1 }; });
  var stdev = scores.length > 1 ? stdDev(scores) : 0;

  var level = stdev < 0.5 ? 'low' : stdev < 1.2 ? 'medium' : 'high';
  var pct = Math.min(100, Math.round((stdev / 2) * 100));

  var meterFill = document.getElementById('meter-fill');
  meterFill.className = 'meter-fill ' + level;
  requestAnimationFrame(function() {
    meterFill.style.width = pct + '%';
  });

  document.getElementById('meter-stdev').textContent = 'Std dev ' + stdev.toFixed(2);

  var sorted = personas.slice().sort(function(a, b) { return a.stance_score - b.stance_score; });
  var clusters = clusterByAgreement(sorted);
  var clusterText = clusters.length + ' cluster' + (clusters.length !== 1 ? 's' : '') + ' detected: ' +
    clusters.map(function(c) { return c.length + ' persona' + (c.length !== 1 ? 's' : ''); }).join(', ');
  document.getElementById('meter-clusters').textContent = clusterText;

  return { sorted: sorted, clusters: clusters };
}

// ── Render: 2D Axis Grid ──

function renderAxisGrid(personas) {
  var grid = document.getElementById('axis-grid');
  // Remove old dots + unavailable message
  grid.querySelectorAll('.axis-dot, .axis-unavailable').forEach(function(d) { d.remove(); });
  grid.querySelectorAll('.gridline-h, .gridline-v, .axis-center-h, .axis-center-v').forEach(function(d) { d.remove(); });

  var withAxes = personas.filter(function(p) { return p.axes; });

  // No axes data at all — show unavailable message
  if (withAxes.length === 0) {
    var msg = document.createElement('div');
    msg.className = 'axis-unavailable';
    msg.textContent = 'Axis data unavailable for this analysis';
    grid.appendChild(msg);
    return;
  }

  // Gridlines
  [0, 25, 50, 75, 100].forEach(function(p) {
    var h = document.createElement('div');
    h.className = 'gridline-h';
    h.style.top = p + '%';
    grid.appendChild(h);

    var v = document.createElement('div');
    v.className = 'gridline-v';
    v.style.left = p + '%';
    grid.appendChild(v);
  });

  // Center axes
  var ch = document.createElement('div');
  ch.className = 'axis-center-h';
  grid.appendChild(ch);
  var cv = document.createElement('div');
  cv.className = 'axis-center-v';
  grid.appendChild(cv);

  // Plot dots
  var toPct = function(v) { return ((clamp(v, -3, 3) + 3) / 6) * 100; };

  withAxes.forEach(function(p) {
    var econ = p.axes.economic;
    var soc = p.axes.social;
    var bg = colourForAxes(econ, soc);

    var dot = document.createElement('div');
    dot.className = 'axis-dot';
    dot.style.left = 'calc(' + toPct(econ) + '% - 7px)';
    dot.style.top = 'calc(' + (100 - toPct(soc)) + '% - 7px)';
    dot.style.background = bg;
    dot.title = p.title + ': econ ' + econ.toFixed(1) + ', social ' + soc.toFixed(1);
    grid.appendChild(dot);
  });

  // Partial axes notice — some personas missing data
  if (withAxes.length < personas.length) {
    var notice = document.createElement('div');
    notice.className = 'axis-unavailable axis-partial';
    notice.textContent = withAxes.length + ' of ' + personas.length + ' personas provided axis data';
    grid.appendChild(notice);
  }

  // Legend dots
  document.getElementById('legend-dot-lo').style.background = colourForAxes(-3, -3);
  document.getElementById('legend-dot-hi').style.background = colourForAxes(3, 3);
}

// ── Render: Persona Card ──

function buildPersonaCard(p) {
  var card = document.createElement('div');
  card.className = 'persona-card';

  var conf = Math.round((p.confidence || 0) * 100);
  var stanceText = (p.stance_score >= 0 ? '+' : '') + p.stance_score.toFixed(1);

  var html = '<div class="persona-card-header">' +
    '<h4 class="persona-title">' + escapeHtml(p.title) + '</h4>' +
    '<span class="persona-badge">Score ' + stanceText + ' &middot; Conf ' + conf + '%</span>' +
  '</div>' +
  '<p class="persona-summary">' + escapeHtml(p.summary) + '</p>';

  // Key claims
  if (p.key_claims && p.key_claims.length > 0) {
    html += '<div class="persona-section-title">Key claims</div>' +
      '<ul class="persona-claims">';
    p.key_claims.forEach(function(c) {
      html += '<li>' + escapeHtml(c) + '</li>';
    });
    html += '</ul>';
  }

  // Fact checks
  if (p.fact_checks && p.fact_checks.length > 0) {
    html += '<div class="persona-section-title">Fact checks</div>' +
      '<ul class="fact-check-list">';
    p.fact_checks.forEach(function(fc) {
      html += '<li class="fact-check-item">' +
        '<div class="fact-check-claim">' + escapeHtml(fc.claim) + '</div>' +
        '<div class="fact-check-detail">' +
          '<span class="assessment-badge ' + escapeHtml(fc.assessment) + '">' + escapeHtml(fc.assessment) + '</span> &middot; ' +
          escapeHtml(fc.rationale) +
        '</div>' +
      '</li>';
    });
    html += '</ul>';
  }

  // Caveats
  if (p.caveats && p.caveats.length > 0) {
    html += '<div class="persona-section-title">Caveats</div>' +
      '<ul class="persona-caveats">';
    p.caveats.forEach(function(c) {
      html += '<li>' + escapeHtml(c) + '</li>';
    });
    html += '</ul>';
  }

  card.innerHTML = html;
  return card;
}

// ── Render: Full Results ──

function renderResults(rawData, sourceUrl) {
  var data = normaliseResponse(rawData);
  currentData = data;

  // Article info
  document.getElementById('article-title').textContent = data.title;
  var sourceLink = document.getElementById('article-source');
  var url = sourceUrl || data.source_url;
  if (url && url.startsWith('http')) {
    sourceLink.textContent = url;
    sourceLink.href = safeHref(url);
    sourceLink.style.display = '';
  } else {
    sourceLink.style.display = 'none';
  }

  // Summary text: prefer raw article_summary (legacy), fall back to debiaser summary
  var summaryText = rawData.article_summary || data.debiaser.truth_seeking_summary || '';
  document.getElementById('article-summary-text').textContent = summaryText;

  // Spectrum bar
  renderSpectrum(data.debiaser.spectrum_score || 0);

  // Disagreement meter + clustering
  var clusterInfo = renderDisagreementMeter(data.personas);

  // 2D toggle — always render (shows unavailable message if no axes data)
  var show2dCheckbox = document.getElementById('show-2d');
  var axisSection = document.getElementById('axis-grid-section');
  renderAxisGrid(data.personas);
  if (show2dCheckbox.checked) show(axisSection);

  // Partial results notice — show API warnings or persona count note
  var partialNotice = document.getElementById('partial-notice');
  var warnings = rawData.warnings || [];
  var totalPersonas = data.personas.length;
  if (warnings.length > 0) {
    partialNotice.textContent = warnings.join(' \u2022 ');
    show(partialNotice);
  } else if (totalPersonas > 0 && totalPersonas < 8) {
    var failed = 8 - totalPersonas;
    partialNotice.textContent = 'Partial results: ' + totalPersonas + ' of 8 personas responded. ' +
      failed + ' persona' + (failed !== 1 ? 's' : '') + ' failed to analyse this article. ' +
      'Results below are based on available perspectives only.';
    show(partialNotice);
  } else {
    hide(partialNotice);
  }

  // Persona clusters
  var clustersContainer = document.getElementById('persona-clusters');
  clustersContainer.innerHTML = '';
  clusterInfo.clusters.forEach(function(cluster, idx) {
    var min = Math.min.apply(null, cluster.map(function(p) { return p.stance_score; }));
    var max = Math.max.apply(null, cluster.map(function(p) { return p.stance_score; }));
    var span = (max - min).toFixed(2);

    var block = document.createElement('section');
    block.className = 'cluster-block';

    var meta = document.createElement('div');
    meta.className = 'cluster-meta';
    meta.textContent = 'Cluster ' + (idx + 1) + ' \u00B7 ' + cluster.length + ' persona' + (cluster.length !== 1 ? 's' : '') + ' \u00B7 span ' + span;
    block.appendChild(meta);

    var grid = document.createElement('div');
    grid.className = 'cluster-cards';
    cluster.forEach(function(p) {
      grid.appendChild(buildPersonaCard(p));
    });
    block.appendChild(grid);

    clustersContainer.appendChild(block);
  });

  // Debiaser section
  document.getElementById('debiaser-summary').textContent = data.debiaser.truth_seeking_summary || '';

  var consensusList = document.getElementById('debiaser-consensus');
  var disagreementsList = document.getElementById('debiaser-disagreements');
  var biasList = document.getElementById('debiaser-bias');

  consensusList.innerHTML = '';
  (data.debiaser.consensus_points || []).forEach(function(item) {
    var li = document.createElement('li');
    li.textContent = item;
    consensusList.appendChild(li);
  });

  disagreementsList.innerHTML = '';
  (data.debiaser.disagreements || []).forEach(function(item) {
    var li = document.createElement('li');
    li.textContent = item;
    disagreementsList.appendChild(li);
  });

  biasList.innerHTML = '';
  (data.debiaser.likely_bias_drivers || []).forEach(function(item) {
    var li = document.createElement('li');
    li.textContent = item;
    biasList.appendChild(li);
  });

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

    hide(resultsArea);
    hide(compareResults);
    hideError();
  });
});

// ── 2D Toggle ──

document.getElementById('show-2d').addEventListener('change', function() {
  var section = document.getElementById('axis-grid-section');
  if (this.checked && currentData) {
    show(section);
  } else {
    hide(section);
  }
});

// ── Copy to Clipboard ──

function copyToClipboard(text, btn, labelEl) {
  navigator.clipboard.writeText(text).then(function() {
    btn.classList.add('copied');
    if (labelEl) labelEl.textContent = 'Copied';
    setTimeout(function() {
      btn.classList.remove('copied');
      if (labelEl) labelEl.textContent = 'Copy';
    }, 2000);
  }).catch(function() {
    var ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    document.execCommand('copy');
    document.body.removeChild(ta);
    btn.classList.add('copied');
    if (labelEl) labelEl.textContent = 'Copied';
    setTimeout(function() {
      btn.classList.remove('copied');
      if (labelEl) labelEl.textContent = 'Copy';
    }, 2000);
  });
}

// ── History (localStorage) ──

var HISTORY_KEY = 'politicaldebaiser_history';
var MAX_HISTORY = 50;

function getHistory() {
  try {
    return JSON.parse(localStorage.getItem(HISTORY_KEY)) || [];
  } catch (e) { return []; }
}

function saveHistory(history) {
  localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
}

function addToHistory(title, url, rawData) {
  var history = getHistory();
  var entry = {
    id: generateId(),
    title: title,
    url: url || '',
    timestamp: Date.now(),
    data: rawData
  };
  history.unshift(entry);
  if (history.length > MAX_HISTORY) history = history.slice(0, MAX_HISTORY);
  saveHistory(history);
  renderHistory();
  return entry.id;
}

function deleteFromHistory(id) {
  var history = getHistory().filter(function(h) { return h.id !== id; });
  saveHistory(history);
  renderHistory();
}

function clearHistory() {
  localStorage.removeItem(HISTORY_KEY);
  renderHistory();
}

function renderHistory() {
  var history = getHistory();
  if (history.length === 0) {
    historyList.innerHTML = '<div class="history-empty">No analysis history yet.</div>';
    return;
  }
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
      renderResults(item.data, item.url);
      closeSidebar();
      // Switch to URL tab
      document.querySelectorAll('.input-tab').forEach(function(t) { t.classList.remove('active'); });
      document.querySelector('[data-tab="url"]').classList.add('active');
      currentMode = 'url';
      form.classList.remove('hidden');
      document.getElementById('text-input-area').classList.remove('active');
      document.getElementById('compare-input-area').classList.remove('active');
      hide(compareResults);
    });

    el.querySelector('.history-item-delete').addEventListener('click', function(e) {
      e.stopPropagation();
      deleteFromHistory(item.id);
    });

    historyList.appendChild(el);
  });
}

// ── Sidebar Toggle ──

function openSidebar() {
  sidebar.classList.add('open');
  sidebarOverlay.classList.add('open');
  renderHistory();
}

function closeSidebar() {
  sidebar.classList.remove('open');
  sidebarOverlay.classList.remove('open');
}

document.getElementById('sidebar-toggle').addEventListener('click', openSidebar);
document.getElementById('sidebar-close').addEventListener('click', closeSidebar);
sidebarOverlay.addEventListener('click', closeSidebar);
document.getElementById('clear-history').addEventListener('click', function() {
  clearHistory();
});

// ── Export Functions ──

function downloadFile(content, filename, type) {
  var blob = new Blob([content], { type: type });
  var u = URL.createObjectURL(blob);
  var a = document.createElement('a');
  a.href = u;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(u);
}

document.getElementById('export-json').addEventListener('click', function() {
  if (!currentData) return;
  var json = JSON.stringify(currentData._raw || currentData, null, 2);
  var title = (currentData.title || 'analysis').replace(/[^a-z0-9]/gi, '_').substring(0, 40);
  downloadFile(json, title + '.json', 'application/json');
});

document.getElementById('export-text').addEventListener('click', function() {
  if (!currentData) return;
  var lines = [];
  lines.push('Debiaser Analysis Report');
  lines.push('='.repeat(40));
  lines.push('');
  lines.push('Article: ' + currentData.title);
  if (lastUrl) lines.push('Source: ' + lastUrl);
  lines.push('');

  if (currentData.debiaser.truth_seeking_summary) {
    lines.push('Truth-Seeking Summary:');
    lines.push(currentData.debiaser.truth_seeking_summary);
    lines.push('');
  }

  lines.push('Spectrum Score: ' + (currentData.debiaser.spectrum_score || 0).toFixed(2));
  lines.push('');

  currentData.personas.forEach(function(p) {
    var stanceText = (p.stance_score >= 0 ? '+' : '') + p.stance_score.toFixed(1);
    var conf = Math.round((p.confidence || 0) * 100);
    lines.push('-'.repeat(40));
    lines.push(p.title + ' (Stance: ' + stanceText + ', Confidence: ' + conf + '%)');
    lines.push('');
    lines.push(p.summary);
    lines.push('');
    if (p.key_claims && p.key_claims.length > 0) {
      lines.push('Key Claims:');
      p.key_claims.forEach(function(c) { lines.push('  - ' + c); });
      lines.push('');
    }
    if (p.fact_checks && p.fact_checks.length > 0) {
      lines.push('Fact Checks:');
      p.fact_checks.forEach(function(fc) {
        lines.push('  [' + fc.assessment + '] ' + fc.claim + ' \u2014 ' + fc.rationale);
      });
      lines.push('');
    }
    if (p.caveats && p.caveats.length > 0) {
      lines.push('Caveats:');
      p.caveats.forEach(function(c) { lines.push('  - ' + c); });
      lines.push('');
    }
  });

  if (currentData.debiaser.consensus_points && currentData.debiaser.consensus_points.length > 0) {
    lines.push('='.repeat(40));
    lines.push('Consensus:');
    currentData.debiaser.consensus_points.forEach(function(c) { lines.push('  - ' + c); });
    lines.push('');
  }

  if (currentData.debiaser.disagreements && currentData.debiaser.disagreements.length > 0) {
    lines.push('Disagreements:');
    currentData.debiaser.disagreements.forEach(function(c) { lines.push('  - ' + c); });
    lines.push('');
  }

  if (currentData.debiaser.likely_bias_drivers && currentData.debiaser.likely_bias_drivers.length > 0) {
    lines.push('Likely Bias Drivers:');
    currentData.debiaser.likely_bias_drivers.forEach(function(c) { lines.push('  - ' + c); });
    lines.push('');
  }

  var title = (currentData.title || 'analysis').replace(/[^a-z0-9]/gi, '_').substring(0, 40);
  downloadFile(lines.join('\n'), title + '.txt', 'text/plain');
});

document.getElementById('share-link').addEventListener('click', function() {
  if (!currentData) return;
  var history = getHistory();
  var found = history.find(function(h) { return h.title === currentData.title; });
  var id;
  if (found) {
    id = found.id;
  } else {
    id = addToHistory(currentData.title, lastUrl, currentData._raw || currentData);
  }
  var shareUrl = window.location.origin + window.location.pathname + '#/history/' + id;
  var btn = document.getElementById('share-link');
  copyToClipboard(shareUrl, btn, null);
  btn.textContent = 'Link Copied!';
  setTimeout(function() { btn.textContent = 'Share Link'; }, 2000);
});

// ── API: Analyse URL ──

async function doAnalyze(url) {
  setLoading(true);
  lastUrl = url;

  try {
    var res = await fetch('/analyze', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ url: url })
    });

    if (!res.ok) {
      var err = await parseApiError(res);
      throw err;
    }

    var data = await res.json();
    setLoading(false);
    renderResults(data, url);
    addToHistory(data.article_title || data.title || 'Untitled', url, data);
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

// ── API: Analyse Text ──

document.getElementById('text-analyze-btn').addEventListener('click', async function() {
  var text = document.getElementById('text-content-input').value.trim();
  if (!text) return;

  var title = document.getElementById('text-title-input').value.trim() || 'Untitled Text';

  setLoading(true);
  lastUrl = '';

  try {
    var res = await fetch('/analyze-text', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: text, title: title })
    });

    if (!res.ok) {
      var err = await parseApiError(res);
      throw err;
    }

    var data = await res.json();
    setLoading(false);
    renderResults(data, '');
    addToHistory(data.article_title || data.title || title, '', data);
  } catch (err) {
    setLoading(false);
    showError(classifyError(err));
  }
});

// ── API: Compare ──

document.getElementById('compare-btn').addEventListener('click', async function() {
  var urlA = document.getElementById('compare-url-a').value.trim();
  var urlB = document.getElementById('compare-url-b').value.trim();
  if (!urlA || !urlB) return;

  setLoading(true);
  hide(compareResults);

  try {
    var results = await Promise.all([
      fetch('/analyze', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: urlA })
      }),
      fetch('/analyze', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: urlB })
      })
    ]);

    var resA = results[0];
    var resB = results[1];

    if (!resA.ok) { var err = await parseApiError(resA); throw err; }
    if (!resB.ok) { var err = await parseApiError(resB); throw err; }

    var dataA = await resA.json();
    var dataB = await resB.json();

    setLoading(false);
    renderCompareResults(dataA, urlA, dataB, urlB);
    addToHistory(dataA.article_title || dataA.title || 'Article A', urlA, dataA);
    addToHistory(dataB.article_title || dataB.title || 'Article B', urlB, dataB);
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
  colA.innerHTML = '';
  colB.innerHTML = '';

  // Column A
  colA.innerHTML = '<div class="compare-col-header">' + escapeHtml(dataA.title) + '</div>' +
    '<div class="article-info-card" style="margin-bottom:1rem">' +
      '<a class="article-source" href="' + escapeHtml(safeHref(urlA)) + '" target="_blank" rel="noopener noreferrer">' + escapeHtml(getDomain(urlA)) + '</a>' +
      '<p class="article-summary-text">Spectrum: ' + (dataA.debiaser.spectrum_score || 0).toFixed(2) + '</p>' +
    '</div>';
  dataA.personas.forEach(function(p) {
    colA.appendChild(buildPersonaCard(p));
  });

  // Column B
  colB.innerHTML = '<div class="compare-col-header">' + escapeHtml(dataB.title) + '</div>' +
    '<div class="article-info-card" style="margin-bottom:1rem">' +
      '<a class="article-source" href="' + escapeHtml(safeHref(urlB)) + '" target="_blank" rel="noopener noreferrer">' + escapeHtml(getDomain(urlB)) + '</a>' +
      '<p class="article-summary-text">Spectrum: ' + (dataB.debiaser.spectrum_score || 0).toFixed(2) + '</p>' +
    '</div>';
  dataB.personas.forEach(function(p) {
    colB.appendChild(buildPersonaCard(p));
  });

  hide(resultsArea);
  show(compareResults);
}

// ── Retry Button ──

document.getElementById('retry-btn').addEventListener('click', function() {
  hideError();
  if (currentMode === 'url') {
    var url = lastUrl || urlInput.value.trim();
    if (url) doAnalyze(url);
  } else if (currentMode === 'text') {
    document.getElementById('text-analyze-btn').click();
  } else if (currentMode === 'compare') {
    document.getElementById('compare-btn').click();
  }
});

// ── URL Hash Routing (Share Links) ──

function handleHashRoute() {
  var hash = window.location.hash;
  if (!hash.startsWith('#/history/')) return;
  var id = hash.replace('#/history/', '');
  var history = getHistory();
  var entry = history.find(function(h) { return h.id === id; });
  if (entry) {
    renderResults(entry.data, entry.url);
  }
}

window.addEventListener('hashchange', handleHashRoute);
handleHashRoute();

// ── Health Check ──

var healthDot = document.getElementById('health-dot');
var healthLabel = document.getElementById('health-label');
var healthOk = false;

function checkHealth() {
  fetch('/health').then(function(res) {
    if (res.ok) {
      healthDot.className = 'health-dot ok';
      healthLabel.textContent = 'Connected';
      healthDot.parentElement.title = 'Server is reachable';
      healthOk = true;
    } else {
      setHealthError();
    }
  }).catch(function() {
    setHealthError();
  });
}

function setHealthError() {
  healthDot.className = 'health-dot error';
  healthLabel.textContent = 'Disconnected';
  healthDot.parentElement.title = 'Cannot reach server — is it running?';
  healthOk = false;
}

// Poll health every 30s, check immediately on load
checkHealth();
setInterval(checkHealth, 30000);

// ── Init ──

renderHistory();
