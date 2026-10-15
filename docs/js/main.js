// Main Documentation Interactions & Tab Switchers

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

// Highlight active sidebar links on scroll
window.addEventListener('DOMContentLoaded', () => {
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
