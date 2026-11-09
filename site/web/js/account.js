import { onAuthStateChanged, signOut } from 'firebase/auth';
import { api } from './lib/api.js';
import { getFirebaseAuth } from './lib/firebase.js';
import { completeEmailLink, renderSignIn } from './lib/signin.js';
import { $, $$, SITE, banner, date, esc, toast, won } from './lib/site.js';

const params = new URLSearchParams(location.search);

const ORDER_STATUS = {
  paid: '결제 완료',
  waiting: '입금 대기',
  canceled: '환불됨',
  partially_canceled: '부분 환불',
  failed: '실패',
};

function deviceList(l) {
  if (!l.devices.length) return '<p class="muted small" style="margin:0">아직 등록한 컴퓨터가 없어요.</p>';
  return `<ul class="devices">${l.devices
    .map((d) => {
      const meta = [d.os, d.appVersion && `v${d.appVersion}`, d.lastAt && `최근 확인 ${date(d.lastAt)}`].filter(Boolean).join(' · ');
      return `<li><span><b>${esc(d.name ?? '이름 없는 컴퓨터')}</b><br><span class="meta">${esc(meta)}</span></span>
        <button class="btn btn-line btn-sm" type="button" data-remove-license="${esc(l.id)}" data-remove-device="${esc(d.id)}">빼기</button></li>`;
    })
    .join('')}</ul>`;
}

function licenseCard(l) {
  const active = l.status === 'active';
  return `
    <section class="panel license">
      <div style="display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap">
        <h2 style="margin:0">LOUPE 라이선스</h2>
        <span class="status${active ? '' : ' off'}">${active ? '사용 중' : '해지됨'}</span>
      </div>
      <div class="license-key">
        <code>${esc(l.key)}</code>
        <button class="btn btn-soft btn-sm" type="button" data-copy="${esc(l.key)}"><svg width="16" height="16" aria-hidden="true"><use href="#i-copy"/></svg>복사</button>
      </div>
      <div class="license-meta">
        <span>구매 ${esc(date(l.createdAt))}</span>
        <span>새 버전 무료 ${esc(date(l.updatesUntil))}까지</span>
        <span>컴퓨터 ${l.devices.length} / ${l.maxDevices}대</span>
      </div>
      <div><h3 class="small" style="margin:0 0 8px;color:var(--ink-2)">등록한 컴퓨터</h3>${deviceList(l)}</div>
    </section>`;
}

const howTo = () =>
  SITE.appActivation
    ? `<section class="panel"><h2>앱에서 쓰는 법</h2><ol class="muted" style="margin:0;padding-left:20px">
        <li>LOUPE를 열고 <b>설정 → 라이선스</b>로 가세요.</li>
        <li>위의 라이선스 키를 붙여 넣고 <b>등록</b>을 누르세요.</li>
        <li>컴퓨터를 바꿀 때는 이 페이지에서 쓰지 않는 컴퓨터를 빼 주세요.</li></ol></section>`
    : `<section class="panel"><h2>앱에서 쓰는 법</h2><p class="muted" style="margin:0">앱에 키를 넣는 기능은 다음 업데이트에서 열려요. 그때까지 모든 기능을 그대로 쓰실 수 있고, 키는 이 페이지에 안전하게 보관됩니다.</p></section>`;

const downloads = () => `
  <section class="panel"><h2>내려받기</h2><div class="dl-row">
    <a class="btn btn-soft btn-sm" href="${esc(SITE.downloads.mac)}"><svg width="16" height="16" aria-hidden="true"><use href="#i-download"/></svg>macOS</a>
    <a class="btn btn-soft btn-sm" href="${esc(SITE.downloads.windows)}"><svg width="16" height="16" aria-hidden="true"><use href="#i-download"/></svg>Windows</a>
    <a class="btn btn-soft btn-sm" href="${esc(SITE.downloads.linux)}"><svg width="16" height="16" aria-hidden="true"><use href="#i-download"/></svg>Linux</a>
  </div></section>`;

function orders(list) {
  if (!list.length) return '';
  const rows = list
    .map(
      (o) => `<tr>
        <td>${esc(date(o.paidAt ?? o.createdAt))}</td>
        <td class="hide-sm">${esc(o.orderName)}<br><span class="muted small">${esc(o.id)}</span></td>
        <td>${esc(won(o.amount))}</td>
        <td>${esc(ORDER_STATUS[o.status] ?? o.status)}${o.mode !== 'live' ? ' <span class="muted small">(테스트)</span>' : ''}</td>
        <td>${o.receiptUrl ? `<a href="${esc(o.receiptUrl)}" target="_blank" rel="noopener">영수증</a>` : ''}</td>
      </tr>`,
    )
    .join('');
  return `<section class="panel"><h2>주문 내역</h2><div style="overflow-x:auto"><table class="orders">
    <thead><tr><th>날짜</th><th class="hide-sm">상품 · 주문번호</th><th>금액</th><th>상태</th><th></th></tr></thead>
    <tbody>${rows}</tbody></table></div></section>`;
}

const noLicense = `
  <section class="panel"><div class="empty">
    <img src="/assets/img/icon-512.png" alt="" width="88" height="88">
    <b>아직 라이선스가 없어요</b>
    <span class="small">먼저 무료로 써 보고, 마음에 들면 구매하세요.</span>
    <a class="btn btn-primary" href="/checkout">라이선스 구매</a>
  </div></section>`;

async function load(user, auth) {
  const root = $('[data-account]');
  let me;
  try {
    me = await api('/me', { user });
  } catch (e) {
    root.innerHTML = `<section class="panel"><p style="margin:0">${esc(e.message)}</p></section>`;
    return;
  }
  if (params.get('paid')) banner('ok', '결제가 끝났어요. 고맙습니다!', ' 라이선스 키가 준비됐어요. 아래에서 복사해 두세요.');
  else if (params.get('order')) banner('info', '입금을 기다리고 있어요.', ' 입금이 확인되면 라이선스가 자동으로 발급돼요.');

  root.innerHTML = `
    <div class="who" style="margin-bottom:18px"><span>${esc(me.email ?? user.email ?? '')}</span><button type="button" data-signout>로그아웃</button></div>
    ${me.licenses.length ? me.licenses.map(licenseCard).join('') : noLicense}
    ${me.licenses.length ? howTo() : ''}
    ${downloads()}
    ${orders(me.orders)}`;

  $('[data-signout]', root).addEventListener('click', () => signOut(auth));
  $$('[data-copy]', root).forEach((b) =>
    b.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(b.dataset.copy);
        toast('라이선스 키를 복사했어요.');
      } catch {
        toast('복사하지 못했어요. 키를 길게 눌러 직접 복사해 주세요.');
      }
    }),
  );
  $$('[data-remove-device]', root).forEach((b) =>
    b.addEventListener('click', async () => {
      if (!window.confirm('이 컴퓨터를 라이선스에서 뺄까요? 그 컴퓨터에서 다시 쓰려면 키를 다시 등록해야 해요.')) return;
      b.disabled = true;
      try {
        await api(`/me/licenses/${encodeURIComponent(b.dataset.removeLicense)}/devices/${encodeURIComponent(b.dataset.removeDevice)}/remove`, {
          method: 'POST',
          user,
        });
        toast('컴퓨터를 뺐어요.');
        await load(user, auth);
      } catch (e) {
        toast(e.message);
        b.disabled = false;
      }
    }),
  );
}

function signedOut(auth) {
  const root = $('[data-account]');
  root.innerHTML = `<section class="panel"><h2>로그인</h2><p class="muted" style="margin-top:0">구매할 때 쓴 계정으로 로그인하면 라이선스 키와 주문 내역이 보여요.</p><div data-signin></div></section>`;
  renderSignIn($('[data-signin]', root), auth, { next: '/account' });
}

async function main() {
  const auth = await getFirebaseAuth();
  try {
    const next = await completeEmailLink(auth);
    if (next && next !== '/account') {
      location.replace(next);
      return;
    }
    if (next) history.replaceState(null, '', '/account');
  } catch {
    banner('error', '로그인 링크가 만료되었거나 이미 쓰였어요.', ' 다시 로그인해 주세요.');
    history.replaceState(null, '', '/account');
  }
  onAuthStateChanged(auth, (user) => (user ? load(user, auth) : signedOut(auth)));
}

main();
