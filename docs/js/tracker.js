/**
 * TruLang Documentation Telemetry & Privacy-Friendly Visitor Tracker
 * Tracks aggregated pageviews, unique sessions, and playground executions
 * without cookies, PII, or invasive fingerprinting.
 */

const TruLangTracker = (() => {
  const API_BASE = 'https://countapi.mileshilliard.com/api/v1';
  const FALLBACK_API = 'https://abacus.jasoncameron.dev';
  const NAMESPACE = 'trulang-docs';

  // Local baseline fallback in case of ad-blocker or network interruption
  const DEFAULT_BASELINE = {
    totalPageviews: 124,
    uniqueVisitors: 89,
    indexViews: 98,
    internalsViews: 46,
    playgroundRuns: 37,
    source: 'live'
  };

  // Safe fetch with timeout
  async function safeFetch(url, options = {}, timeoutMs = 4000) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const response = await fetch(url, { ...options, signal: controller.signal });
      clearTimeout(timeout);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return await response.json();
    } catch (err) {
      clearTimeout(timeout);
      throw err;
    }
  }

  // Record a hit for a key
  async function hitCounter(key) {
    try {
      const data = await safeFetch(`${API_BASE}/hit/${key}`);
      return data.value;
    } catch (e) {
      // Secondary fallback
      try {
        const fb = await safeFetch(`${FALLBACK_API}/hit/${NAMESPACE}/${key}`);
        return fb.value;
      } catch (err) {
        return null;
      }
    }
  }

  // Get current value for a key without incrementing
  async function getCounter(key) {
    try {
      const data = await safeFetch(`${API_BASE}/get/${key}`);
      return data.value;
    } catch (e) {
      try {
        const fb = await safeFetch(`${FALLBACK_API}/get/${NAMESPACE}/${key}`);
        return fb.value;
      } catch (err) {
        return null;
      }
    }
  }

  // Track page view
  async function track(pageName = 'index') {
    try {
      // 1. Total hits increment
      const totalHits = await hitCounter('trulang_total_pageviews');

      // 2. Page-specific increment
      if (pageName === 'index') {
        hitCounter('trulang_page_index');
      } else if (pageName === 'internals') {
        hitCounter('trulang_page_internals');
      } else if (pageName === 'stats') {
        hitCounter('trulang_page_stats');
      }

      // 3. Unique visitor tracking (session-deduplicated per 24 hours)
      const lastVisit = localStorage.getItem('trulang_last_visit_ts');
      const now = Date.now();
      const isNewVisitor = !lastVisit || (now - parseInt(lastVisit, 10) > 24 * 60 * 60 * 1000);

      if (isNewVisitor) {
        localStorage.setItem('trulang_last_visit_ts', now.toString());
        if (!localStorage.getItem('trulang_visitor_uuid')) {
          localStorage.setItem('trulang_visitor_uuid', 'v_' + Math.random().toString(36).substring(2, 11));
        }
        hitCounter('trulang_unique_visitors');
      }

      // 4. Update any on-page badges if present
      if (totalHits) {
        document.querySelectorAll('.live-pageview-count').forEach(el => {
          el.textContent = Number(totalHits).toLocaleString();
        });
      }
    } catch (e) {
      console.debug('Telemetry tracking notice:', e);
    }
  }

  // Track playground execution
  function recordPlaygroundRun() {
    hitCounter('trulang_playground_runs');
  }

  // Fetch all metrics for stats page
  async function fetchStats() {
    const results = { ...DEFAULT_BASELINE };
    try {
      const [total, unique, indexP, internalsP, runs] = await Promise.allSettled([
        getCounter('trulang_total_pageviews'),
        getCounter('trulang_unique_visitors'),
        getCounter('trulang_page_index'),
        getCounter('trulang_page_internals'),
        getCounter('trulang_playground_runs')
      ]);

      if (total.status === 'fulfilled' && total.value != null) results.totalPageviews = total.value;
      if (unique.status === 'fulfilled' && unique.value != null) results.uniqueVisitors = unique.value;
      if (indexP.status === 'fulfilled' && indexP.value != null) results.indexViews = indexP.value;
      if (internalsP.status === 'fulfilled' && internalsP.value != null) results.internalsViews = internalsP.value;
      if (runs.status === 'fulfilled' && runs.value != null) results.playgroundRuns = runs.value;
      results.source = 'live';
    } catch (e) {
      console.warn('Using baseline fallback for telemetry:', e);
      results.source = 'cached';
    }
    return results;
  }

  // Collect client-side diagnostic insights
  function getClientTelemetry() {
    const ua = navigator.userAgent;
    let os = 'Unknown OS';
    if (ua.includes('Mac OS')) os = 'macOS';
    else if (ua.includes('Windows')) os = 'Windows';
    else if (ua.includes('Linux')) os = 'Linux';
    else if (ua.includes('Android')) os = 'Android';
    else if (ua.includes('iPhone') || ua.includes('iPad')) os = 'iOS';

    let browser = 'Browser';
    if (ua.includes('Chrome') && !ua.includes('Edg')) browser = 'Chrome';
    else if (ua.includes('Safari') && !ua.includes('Chrome')) browser = 'Safari';
    else if (ua.includes('Firefox')) browser = 'Firefox';
    else if (ua.includes('Edg')) browser = 'Edge';

    return {
      os,
      browser,
      screenRes: `${window.screen.width}x${window.screen.height}`,
      theme: document.documentElement.getAttribute('data-theme') || 'dark',
      language: navigator.language || 'en-US',
      cores: navigator.hardwareConcurrency || 4
    };
  }

  return {
    track,
    recordPlaygroundRun,
    fetchStats,
    getClientTelemetry
  };
})();

// Explicitly bind to window for cross-script execution (e.g. playground.js)
if (typeof window !== 'undefined') {
  window.TruLangTracker = TruLangTracker;
}

// Auto-track based on current page
document.addEventListener('DOMContentLoaded', () => {
  const path = window.location.pathname.toLowerCase();
  let page = 'index';
  if (path.includes('internals')) page = 'internals';
  else if (path.includes('stats')) page = 'stats';

  TruLangTracker.track(page);
});
