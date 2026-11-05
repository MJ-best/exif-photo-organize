import { api } from './lib/api.js';
import { photoSvg } from './lib/photos.js';
import { $, $$, esc, number, won } from './lib/site.js';

const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

// ---- the desk: photos being culled ----

const TILES = [
  { palette: 0, tower: true, a: true, kind: 'RAW+J' },
  { palette: 1, kind: 'RAW' },
  { palette: 2, defect: 'blur', b: 92, kind: 'RAW+J' },
  { palette: 3, tower: true, kind: 'JPEG' },
  { palette: 4, kind: 'RAW+J' },
  { palette: 5, defect: 'tilt', b: 88, kind: 'RAW' },
  { palette: 6, tower: true, kind: 'RAW+J' },
  { palette: 1, tower: true, a: true, kind: 'RAW+J' },
  { palette: 3, defect: 'shake', b: 85, kind: 'RAW' },
  { palette: 2, kind: 'JPEG' },
  { palette: 4, defect: 'bright', b: 76, kind: 'RAW+J' },
  { palette: 0, a: true, kind: 'RAW' },
];

const STEPS = [
  { title: '새 항목 12장', bar: '<strong>가져오기 완료</strong> · 날짜별 폴더로 정리됨 · RAW+JPEG 6쌍' },
  { title: 'B컷 후보 4장', bar: '<strong>B컷 후보 4장</strong> · 흐림 · 흔들림 · 노출 · 내가 거절한 사진과 비슷' },
  { title: 'A컷 후보 3장', bar: '<strong>A컷 후보 3장</strong> · 가장 잘 나온 상위 15%' },
];

const CHECK = '<svg viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" d="m5 12.5 4.2 4.2L19 7"/></svg>';

function desk() {
  const root = $('[data-desk]');
  const grid = $('[data-grid]');
  if (!root || !grid) return;
  grid.innerHTML = TILES.map(
    (t, i) => `
      <div class="tile${t.a ? ' is-a' : ''}${t.b ? ' is-b' : ''}" style="--i:${i}">
        ${photoSvg(i + 3, t)}
        <span class="kind">${t.kind}</span>
        ${t.b ? `<span class="bprob">B ${t.b}%</span>` : ''}
        ${t.a ? `<span class="stars">★★★★</span><span class="check">${CHECK}</span>` : ''}
      </div>`,
  ).join('');

  const title = $('[data-demo-title]');
  const bar = $('[data-demo-bar]');
  const buttons = $$('[data-step-btn]');
  let step = 0;
  let timer = null;
  let hold = false;

  const show = (n) => {
    step = n;
    root.dataset.step = String(n);
    grid.dataset.step = String(n);
    title.textContent = STEPS[n].title;
    bar.innerHTML = STEPS[n].bar;
    buttons.forEach((b) => b.setAttribute('aria-pressed', String(Number(b.dataset.stepBtn) === n)));
  };
  const tick = () => {
    if (!hold && !document.hidden) show((step + 1) % STEPS.length);
  };
  const start = () => {
    if (!reduceMotion && !timer) timer = setInterval(tick, 3600);
  };
  const stop = () => {
    clearInterval(timer);
    timer = null;
  };

  buttons.forEach((b) =>
    b.addEventListener('click', () => {
      stop();
      show(Number(b.dataset.stepBtn));
      // Let the visitor look; carry on later.
      setTimeout(start, 9000);
    }),
  );
  root.addEventListener('pointerenter', () => (hold = true));
  root.addEventListener('pointerleave', () => (hold = false));
  show(0);
  start();
}

// ---- workflow stepper ----

function flow() {
  const steps = $$('[data-flow-step]');
  const shots = $$('[data-flow-shot]');
  steps.forEach((s) =>
    s.addEventListener('click', () => {
      const n = s.dataset.flowStep;
      steps.forEach((x) => x.setAttribute('aria-selected', String(x === s)));
      shots.forEach((img) => img.classList.toggle('is-active', img.dataset.flowShot === n));
    }),
  );
}

// ---- prices and plan from the server (the HTML has the same defaults) ----

const ICON_CHECK = '<svg aria-hidden="true"><use href="#i-check"/></svg>';

async function catalog() {
  let c;
  try {
    c = await api('/catalog');
  } catch {
    return; // keep what the page says
  }
  const p = c.products?.[0];
  if (!p) return;
  $$('[data-price]').forEach((el) => (el.textContent = won(p.price)));
  const set = (sel, text) => $$(sel).forEach((el) => (el.textContent = text));
  set('[data-plan-price]', number(p.price));
  set('[data-plan-name]', p.name);
  set('[data-plan-note]', p.priceNote);
  if (c.trial) {
    set('[data-trial-title]', c.trial.title);
    set('[data-trial-text]', c.trial.text);
  }
  $$('[data-plan-features]').forEach(
    (ul) => (ul.innerHTML = p.features.map((f) => `<li>${ICON_CHECK}<span>${esc(f)}</span></li>`).join('')),
  );
}

desk();
flow();
catalog();
