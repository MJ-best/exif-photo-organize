// Shared bits for every page: header shadow, reveal on scroll, toast, formatting.

/* global __SITE__ */
export const SITE = __SITE__;

export const $ = (sel, root = document) => root.querySelector(sel);
export const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

const ENTITIES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
export function esc(v) {
  return String(v ?? '').replace(/[&<>"']/g, (c) => ENTITIES[c]);
}

export const won = (n) => `${Number(n).toLocaleString('ko-KR')}원`;
export const number = (n) => Number(n).toLocaleString('ko-KR');

const dateFmt = new Intl.DateTimeFormat('ko-KR', { year: 'numeric', month: 'long', day: 'numeric' });
export const date = (iso) => (iso ? dateFmt.format(new Date(iso)) : '');

let toastTimer;
export function toast(message) {
  const el = $('[data-toast]');
  if (!el) return;
  el.textContent = message;
  el.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove('show'), 2600);
}

export function banner(kind, title, text = '') {
  const el = $('[data-banner]');
  if (!el) return;
  const role = kind === 'ok' || kind === 'info' ? 'status' : 'alert';
  el.innerHTML = title
    ? `<div class="banner ${esc(kind)}" role="${role}"><div><b>${esc(title)}</b>${text ? `<span>${esc(text)}</span>` : ''}</div></div>`
    : '';
}

function header() {
  const h = $('[data-header]');
  if (!h) return;
  const update = () => h.classList.toggle('scrolled', window.scrollY > 8);
  update();
  window.addEventListener('scroll', update, { passive: true });
}

function reveal() {
  const items = $$('.reveal');
  if (!('IntersectionObserver' in window)) {
    items.forEach((el) => el.classList.add('in'));
    return;
  }
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (e.isIntersecting) {
          e.target.classList.add('in');
          io.unobserve(e.target);
        }
      }
    },
    { rootMargin: '0px 0px -8% 0px', threshold: 0.08 },
  );
  items.forEach((el) => io.observe(el));
}

header();
reveal();
