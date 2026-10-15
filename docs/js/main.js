// TruLang Documentation Interactive Logic & Theme Management

// Theme Management (Light / Dark)
function getInitialTheme() {
  const saved = localStorage.getItem('trulang_theme');
  if (saved === 'light' || saved === 'dark') {
    return saved;
  }
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function applyTheme(theme) {
  document.documentElement.setAttribute('data-theme', theme);
  const btn = document.getElementById('themeToggle');
  if (btn) {
    btn.setAttribute('aria-label', `Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`);
    btn.setAttribute('title', `Switch to ${theme === 'dark' ? 'light' : 'dark'} mode`);
  }
}

function toggleTheme() {
  const current = document.documentElement.getAttribute('data-theme') || getInitialTheme();
  const next = current === 'dark' ? 'light' : 'dark';
  localStorage.setItem('trulang_theme', next);
  applyTheme(next);
}

// Sync across browser tabs
window.addEventListener('storage', (e) => {
  if (e.key === 'trulang_theme' && (e.newValue === 'light' || e.newValue === 'dark')) {
    applyTheme(e.newValue);
  }
});

// React to OS preference changes if user hasn't explicitly set a preference
window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
  if (!localStorage.getItem('trulang_theme')) {
    applyTheme(e.matches ? 'dark' : 'light');
  }
});

// Initialize theme on script load
applyTheme(getInitialTheme());

// Tab Switchers
function switchBenchTab(tabKey) {
  const tabs = ['throughput', 'latency', 'memory', 'matrix'];
  tabs.forEach(t => {
    const btn = document.getElementById('btnTab' + t.charAt(0).toUpperCase() + t.slice(1));
    const content = document.getElementById('tabContent' + t.charAt(0).toUpperCase() + t.slice(1));
    if (btn) btn.classList.remove('active');
    if (content) content.classList.remove('active');
  });

  const activeBtn = document.getElementById('btnTab' + tabKey.charAt(0).toUpperCase() + tabKey.slice(1));
  const activeContent = document.getElementById('tabContent' + tabKey.charAt(0).toUpperCase() + tabKey.slice(1));
  if (activeBtn) activeBtn.classList.add('active');
  if (activeContent) activeContent.classList.add('active');
}

function switchInstallTab(tabKey) {
  const tabs = ['source', 'brew', 'binary', 'docker'];
  tabs.forEach(t => {
    const btn = document.getElementById('btnInstall' + t.charAt(0).toUpperCase() + t.slice(1));
    const content = document.getElementById('tabInstall' + t.charAt(0).toUpperCase() + t.slice(1));
    if (btn) btn.classList.remove('active');
    if (content) content.classList.remove('active');
  });

  const activeBtn = document.getElementById('btnInstall' + tabKey.charAt(0).toUpperCase() + tabKey.slice(1));
  const activeContent = document.getElementById('tabInstall' + tabKey.charAt(0).toUpperCase() + tabKey.slice(1));
  if (activeBtn) activeBtn.classList.add('active');
  if (activeContent) activeContent.classList.add('active');
}

function copySnippet(text, btn) {
  navigator.clipboard.writeText(text).then(() => {
    const orig = btn.innerText;
    btn.innerText = 'Copied!';
    setTimeout(() => btn.innerText = orig, 2000);
  });
}

function toggleCard(id) {
  const card = document.getElementById(id);
  if (card) {
    card.classList.toggle('expanded');
  }
}

function copyCode(btn) {
  const pre = btn.closest('.code-box').querySelector('pre code');
  if (pre) {
    navigator.clipboard.writeText(pre.innerText).then(() => {
      btn.innerText = 'Copied!';
      setTimeout(() => btn.innerText = 'Copy', 2000);
    });
  }
}

function filterSpecs() {
  const query = document.getElementById('specSearch').value.toLowerCase();
  const cards = document.querySelectorAll('.spec-card');
  cards.forEach(card => {
    const text = card.innerText.toLowerCase();
    card.style.display = text.includes(query) ? 'block' : 'none';
  });
}

// Highlight active sidebar links on scroll & bind theme button
window.addEventListener('DOMContentLoaded', () => {
  const themeBtn = document.getElementById('themeToggle');
  if (themeBtn) {
    themeBtn.addEventListener('click', toggleTheme);
  }

  const links = document.querySelectorAll('.sidebar-link');
  const sections = document.querySelectorAll('section[id], .spec-card[id]');

  window.addEventListener('scroll', () => {
    let current = '';
    sections.forEach(section => {
      const sectionTop = section.offsetTop - 120;
      if (window.scrollY >= sectionTop) {
        current = section.getAttribute('id');
      }
    });

    if (current) {
      links.forEach(link => {
        link.classList.remove('active');
        if (link.getAttribute('href') === '#' + current) {
          link.classList.add('active');
        }
      });
    }
  });
});
