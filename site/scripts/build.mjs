// Build the site into public/ (what Firebase Hosting serves):
//   - pages from web/*.html, with <!--#include name--> partials and {{config.values}} filled in
//   - JS bundled with esbuild (Firebase SDK included — no third-party scripts until checkout)
//   - fonts (Pretendard, Gowun Batang; SIL OFL) and the app's screenshots and icon, self-hosted
//
//   npm run build

import { build } from 'esbuild';
import { createHash } from 'node:crypto';
import { cp, mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const site = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const repo = path.dirname(site);
const web = path.join(site, 'web');
const out = path.join(site, 'public');
const mod = (p) => path.join(site, 'node_modules', p);

const config = JSON.parse(await readFile(path.join(site, 'site.config.json'), 'utf8'));

await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });

// ---- JS ----
const entries = (await readdir(path.join(web, 'js'))).filter((f) => f.endsWith('.js'));
const js = await build({
  entryPoints: entries.map((f) => path.join(web, 'js', f)),
  bundle: true,
  splitting: true,
  format: 'esm',
  outdir: path.join(out, 'assets/js'),
  entryNames: '[name]',
  chunkNames: 'chunks/[name]-[hash]',
  minify: true,
  target: ['es2020'],
  legalComments: 'none',
  define: { __SITE__: JSON.stringify({ downloads: config.downloads, repo: config.repo, appActivation: config.appActivation, refundDays: config.refundDays }) },
  write: true,
  metafile: true,
  logLevel: 'warning',
});

// ---- static files (everything in web/ except sources and partials) ----
await cp(web, out, {
  recursive: true,
  filter: (src) => {
    const rel = path.relative(web, src);
    return !(rel === 'js' || rel.startsWith(`js${path.sep}`) || rel === 'partials' || rel.startsWith(`partials${path.sep}`));
  },
});

// ---- fonts ----
const fonts = path.join(out, 'assets/fonts');
await cp(mod('pretendard/dist/web/variable/woff2-dynamic-subset'), path.join(fonts, 'pretendard'), { recursive: true });
const pretendardCss = (await readFile(mod('pretendard/dist/web/variable/pretendardvariable-dynamic-subset.css'), 'utf8'))
  .replaceAll('./woff2-dynamic-subset/', './pretendard/');
let gowunCss = '';
await mkdir(path.join(fonts, 'gowun-batang'), { recursive: true });
for (const subset of ['korean', 'latin']) {
  for (const weight of [400, 700]) {
    const file = `gowun-batang-${subset}-${weight}-normal.woff2`;
    await cp(mod(`@fontsource/gowun-batang/files/${file}`), path.join(fonts, 'gowun-batang', file));
    const css = await readFile(mod(`@fontsource/gowun-batang/${subset}-${weight}.css`), 'utf8');
    gowunCss += css.replace(/url\(\.\/files\/([^)]+)\)/g, (m, f) => (f.endsWith('.woff2') ? `url(./gowun-batang/${f})` : m))
      .replace(/,\s*url\([^)]*\.woff\)\s*format\('woff'\)/g, '');
  }
}
await writeFile(path.join(fonts, 'fonts.css'), `${pretendardCss}\n${gowunCss}`);

// ---- the app's screenshots and icon (single source: the repository) ----
await mkdir(path.join(out, 'assets/shots'), { recursive: true });
for (const name of ['library', 'cull', 'taste', 'works', 'import', 'rating', 'projects', 'viewer', 'cocoa', 'filter']) {
  await cp(path.join(repo, 'docs/images', `${name}.jpg`), path.join(out, 'assets/shots', `${name}.jpg`));
}
await mkdir(path.join(out, 'assets/img'), { recursive: true });
for (const name of ['icon-512.png', 'icon-256.png', 'icon-macos-1024.png']) {
  await cp(path.join(repo, 'crates/app/assets', name), path.join(out, 'assets/img', name));
}
await cp(path.join(repo, 'crates/app/assets/icon-256.png'), path.join(out, 'favicon.png'));

// ---- pages ----
const hash = createHash('sha256');
for (const f of Object.keys(js.metafile.outputs).sort()) hash.update(await readFile(path.join(site, f)));
hash.update(await readFile(path.join(web, 'assets/css/site.css')));
const version = hash.digest('hex').slice(0, 10);

const partials = {};
for (const f of await readdir(path.join(web, 'partials'))) {
  partials[f.replace(/\.html$/, '')] = await readFile(path.join(web, 'partials', f), 'utf8');
}
const missing = new Set();
const lookup = (key) => {
  const v = key.split('.').reduce((o, k) => (o == null ? undefined : o[k]), { ...config, v: version });
  if (v === undefined || v === '') {
    missing.add(key);
    return '<span class="todo">［입력 필요］</span>';
  }
  return String(v);
};
for (const f of (await readdir(web)).filter((f) => f.endsWith('.html'))) {
  let html = await readFile(path.join(web, f), 'utf8');
  html = html.replace(/<!--#include (\w+)-->/g, (_, name) => {
    if (!(name in partials)) throw new Error(`${f}: no partial "${name}"`);
    return partials[name];
  });
  html = html.replace(/\{\{\s*([\w.]+)\s*\}\}/g, (_, key) => lookup(key));
  await writeFile(path.join(out, f), html);
}

const size = Object.entries(js.metafile.outputs)
  .map(([f, o]) => `${path.basename(f)} ${(o.bytes / 1024).toFixed(0)}KB`)
  .join(', ');
console.log(`public/ 빌드 완료 (v${version}) — ${size}`);
if (missing.size) {
  console.warn(`⚠ site.config.json 에 비어 있는 값 (공개 전에 채우세요): ${[...missing].join(', ')}`);
}
