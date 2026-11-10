export type ToastType = 'info' | 'success' | 'warn' | 'error';

export function showToast(message: string, type: ToastType = 'info'): void {
  let container = document.getElementById('toast-container');
  if (!container) {
    container = document.createElement('div');
    container.id = 'toast-container';
    container.className = 'fixed bottom-5 right-5 z-[60] flex flex-col space-y-2 pointer-events-none max-w-md';
    document.body.appendChild(container);
  }

  const dotColor =
    type === 'error'
      ? 'bg-rose-400'
      : type === 'warn'
        ? 'bg-amber-400'
        : type === 'success'
          ? 'bg-emerald-400'
          : 'bg-sky-400';

  const borderClass =
    type === 'error'
      ? 'border-rose-500/40'
      : type === 'warn'
        ? 'border-amber-500/40'
        : 'border-slate-700';

  const item = document.createElement('div');
  item.className = `px-4 py-2.5 rounded-lg bg-slate-950/95 border ${borderClass} text-slate-100 text-xs shadow-2xl flex items-center space-x-2.5 transition-all duration-300 transform translate-y-2 opacity-0 pointer-events-auto`;

  const dot = document.createElement('span');
  dot.className = `w-2 h-2 rounded-full shrink-0 ${dotColor}`;
  const textSpan = document.createElement('span');
  textSpan.textContent = message;

  item.appendChild(dot);
  item.appendChild(textSpan);
  container.appendChild(item);

  window.setTimeout(() => {
    item.classList.remove('translate-y-2', 'opacity-0');
  }, 20);
  window.setTimeout(() => {
    item.classList.add('opacity-0', 'translate-y-2');
    window.setTimeout(() => item.remove(), 300);
  }, 3600);
}
