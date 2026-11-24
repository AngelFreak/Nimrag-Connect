// Theme before first paint (keep in step with src/lib/theme.ts): light by default, dark or
// "system" from localStorage, always dark inside the Android app (it injects NimragApp first).
// A file rather than an inline script: the server's CSP is script-src 'self'.
(function () {
  var theme = 'light';
  try {
    var choice = localStorage.getItem('nimrag.theme');
    if (choice === 'dark') theme = 'dark';
    else if (choice === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches) theme = 'dark';
    if (window.NimragApp && window.NimragApp.native === true) theme = 'dark';
  } catch (e) {
    if (window.NimragApp && window.NimragApp.native === true) theme = 'dark';
  }
  document.documentElement.setAttribute('data-theme', theme);
  var meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.setAttribute('content', theme === 'dark' ? '#0F1417' : '#ffffff');
})();
