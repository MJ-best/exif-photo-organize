// Little landscape "photos" drawn as SVG — the same gradient skies, suns and hills as the sample
// library in the app's screenshots — with the faults the culling assistant looks for.

const PALETTES = [
  { sky: ['#ffb36b', '#7a3fa6'], hills: ['#5b2c83', '#3d1d61', '#241339'], sun: '#fff6e3' }, // dusk
  { sky: ['#8fd6ff', '#2477bd'], hills: ['#0f5f9c', '#0b4475', '#082c4d'], sun: '#fffbea' }, // sea
  { sky: ['#b5e6a8', '#4a9a52'], hills: ['#2f7a3a', '#1f5c2b', '#123d1b'], sun: '#fffbea' }, // forest
  { sky: ['#ffe07a', '#f39a35'], hills: ['#c85e1f', '#913f14', '#5c260b'], sun: '#fff7e0' }, // amber
  { sky: ['#ffc6cd', '#dc4d61'], hills: ['#ad2a40', '#7c1c2e', '#51101d'], sun: '#fff3f1' }, // rose
  { sky: ['#cbd3da', '#64717c'], hills: ['#414b55', '#2c343c', '#1c2227'], sun: '#f5f2ea' }, // mist
  { sky: ['#e6c3f2', '#8e4fc1'], hills: ['#5a2d93', '#3e1f6b', '#261245'], sun: '#fff7ea' }, // violet
];

function random(seed) {
  let s = (seed * 2654435761) >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

const f1 = (v) => v.toFixed(1);

const FILTERS = {
  blur: '<feGaussianBlur stdDeviation="2.3"/>',
  shake: '<feGaussianBlur stdDeviation="3.6 0.15"/>',
  bright:
    '<feComponentTransfer><feFuncR type="linear" slope="1.5" intercept="0.32"/><feFuncG type="linear" slope="1.5" intercept="0.32"/><feFuncB type="linear" slope="1.5" intercept="0.32"/></feComponentTransfer>',
};

/** `defect`: blur | shake | bright | tilt (an accidental, crooked frame). */
export function photoSvg(seed, { palette, defect = null, tower = false } = {}) {
  const r = random(seed + 11);
  const p = PALETTES[(palette ?? seed) % PALETTES.length];
  const id = `ph${seed}`;
  const sun = `<circle cx="${f1(16 + r() * 66)}" cy="${f1(14 + r() * 24)}" r="${f1(7 + r() * 9)}" fill="${p.sun}"/>`;
  const hills = p.hills
    .map((c, i) => {
      const y = 50 + i * 15 + r() * 6;
      const a = 5 + r() * 7;
      const k = r() * 16;
      return `<path fill="${c}" d="M0 ${f1(y)} C${f1(18 + k)} ${f1(y - a)} ${f1(42 + k)} ${f1(y + a)} 68 ${f1(y)} S100 ${f1(y - a * 0.6)} 100 ${f1(y)} V100 H0Z"/>`;
    })
    .join('');
  let towerSvg = '';
  if (tower) {
    const x = 22 + r() * 52;
    towerSvg = `<path fill="#1f1712" d="M${f1(x)} 76V47h7v29Z M${f1(x - 3)} 49l6.5-11 6.5 11Z"/>`;
  }
  const filter = FILTERS[defect] ? `<filter id="${id}f" x="-10%" y="-10%" width="120%" height="120%">${FILTERS[defect]}</filter>` : '';
  const tilt = defect === 'tilt' ? ' transform="rotate(24 50 50) scale(1.45)"' : '';
  return (
    `<svg viewBox="0 0 100 100" preserveAspectRatio="xMidYMid slice" aria-hidden="true">` +
    `<defs><linearGradient id="${id}g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${p.sky[0]}"/><stop offset="1" stop-color="${p.sky[1]}"/></linearGradient>${filter}</defs>` +
    `<g${filter ? ` filter="url(#${id}f)"` : ''}><g${tilt}><rect x="-20" y="-20" width="140" height="140" fill="url(#${id}g)"/>${sun}${hills}${towerSvg}</g></g></svg>`
  );
}
