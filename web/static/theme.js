// Apply the saved preference before the first paint, without storing account data.
try {
  const choice = localStorage.getItem('chat.theme');
  const dark = choice === 'dark' || (choice !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  document.documentElement.style.colorScheme = dark ? 'dark' : 'light';
} catch { /* System defaults still work when storage is unavailable. */ }
