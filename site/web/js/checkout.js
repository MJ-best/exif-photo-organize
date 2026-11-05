import { onAuthStateChanged, signOut } from 'firebase/auth';
import { api } from './lib/api.js';
import { getFirebaseAuth } from './lib/firebase.js';
import { renderSignIn } from './lib/signin.js';
import { $, $$, banner, esc, toast, won } from './lib/site.js';

const TOSS_SDK = 'https://js.tosspayments.com/v2/standard';
const CANCELED = new Set(['USER_CANCEL', 'PAY_PROCESS_CANCELED']);
const ICON_CHECK = '<svg aria-hidden="true"><use href="#i-check"/></svg>';

let catalog = null;
let product = null;
let widgets = null;
let currentUser = null;

/** Back from the payment window or from /api/toss/success without a payment. */
function showReturnedError() {
  const params = new URLSearchParams(location.search);
  const code = params.get('error') ?? params.get('code');
  if (!code) return;
  if (CANCELED.has(code)) banner('info', '결제를 취소했어요.', ' 언제든 다시 시도할 수 있어요.');
  else banner('error', '결제가 끝나지 않았어요.', ` ${params.get('message') ?? ''}`);
  history.replaceState(null, '', location.pathname);
}

function loadToss() {
  if (window.TossPayments) return Promise.resolve(window.TossPayments);
  return new Promise((resolve, reject) => {
    const s = document.createElement('script');
    s.src = TOSS_SDK;
    s.onload = () => resolve(window.TossPayments);
    s.onerror = () => reject(new Error('결제 모듈을 불러오지 못했어요. 잠시 뒤 다시 시도해 주세요.'));
    document.head.appendChild(s);
  });
}

function renderProduct() {
  const set = (sel, text) => $$(sel).forEach((el) => (el.textContent = text));
  set('[data-plan-name]', product.name);
  set('[data-plan-note]', product.priceNote);
  set('[data-price]', won(product.price));
  $('[data-pay]').textContent = `${won(product.price)} 결제하기`;
  $('[data-plan-features]').innerHTML = product.features.map((f) => `<li>${ICON_CHECK}<span>${esc(f)}</span></li>`).join('');
}

async function preparePayment() {
  const area = $('[data-pay-area]');
  const pay = $('[data-pay]');
  const { mode, clientKey } = catalog.payments;
  if (mode === 'mock') {
    area.innerHTML =
      '<div class="mock-pay"><b>테스트 결제 (로컬 에뮬레이터)</b><br>토스페이먼츠 대신 가짜 결제로 흐름을 끝까지 확인해요. 돈은 나가지 않습니다.</div>';
    pay.disabled = false;
    return;
  }
  area.innerHTML =
    mode === 'test'
      ? '<div class="banner info"><div><b>테스트 결제예요</b><span> 실제로 돈이 나가지 않는 테스트 키로 결제창을 띄웁니다.</span></div></div>'
      : '';
  const TossPayments = await loadToss();
  widgets = TossPayments(clientKey).widgets({ customerKey: TossPayments.ANONYMOUS });
  await widgets.setAmount({ currency: 'KRW', value: product.price });
  await Promise.all([
    widgets.renderPaymentMethods({ selector: '#payment-method', variantKey: 'DEFAULT' }),
    widgets.renderAgreement({ selector: '#agreement', variantKey: 'AGREEMENT' }),
  ]);
  pay.disabled = false;
}

async function startPayment() {
  const pay = $('[data-pay]');
  pay.disabled = true;
  try {
    // The server sets the amount; the browser only shows it.
    const order = await api('/orders', { method: 'POST', body: { productId: product.id }, user: currentUser });
    if (order.payments.mode === 'mock') {
      const q = new URLSearchParams({ paymentKey: `mock_${order.orderId}`, orderId: order.orderId, amount: String(order.amount) });
      location.assign(`/api/toss/success?${q}`);
      return;
    }
    if (order.amount !== product.price) await widgets.setAmount({ currency: 'KRW', value: order.amount });
    await widgets.requestPayment({
      orderId: order.orderId,
      orderName: order.orderName,
      successUrl: `${location.origin}/api/toss/success`,
      failUrl: `${location.origin}/checkout`,
      customerEmail: order.customerEmail,
      customerName: order.customerName ?? undefined,
      ...(order.taxFreeAmount ? { taxFreeAmount: order.taxFreeAmount } : {}),
    });
  } catch (e) {
    if (CANCELED.has(e.code)) toast('결제를 취소했어요.');
    else banner('error', '결제를 시작하지 못했어요.', ` ${e.message}`);
  } finally {
    pay.disabled = false;
  }
}

async function main() {
  showReturnedError();
  const [auth, c] = await Promise.all([getFirebaseAuth(), api('/catalog').catch(() => null)]);
  if (!c?.products?.length) {
    banner('error', '지금은 결제를 진행할 수 없어요.', ' 잠시 뒤 다시 시도해 주세요.');
    return;
  }
  catalog = c;
  product = c.products[0];
  renderProduct();

  $('[data-pay]').addEventListener('click', startPayment);
  onAuthStateChanged(auth, async (user) => {
    currentUser = user;
    const box = $('[data-signin]');
    $('[data-step-signin]').classList.toggle('done', Boolean(user));
    if (!user) {
      $('[data-pay]').disabled = true;
      renderSignIn(box, auth, { next: '/checkout' });
      return;
    }
    box.innerHTML = `<div class="who"><span>${esc(user.email ?? user.displayName ?? '로그인됨')}</span><button type="button" data-signout>다른 계정으로</button></div>`;
    $('[data-signout]', box).addEventListener('click', () => signOut(auth));
    if (widgets) {
      $('[data-pay]').disabled = false;
      return;
    }
    try {
      await preparePayment();
    } catch (e) {
      banner('error', '결제 수단을 불러오지 못했어요.', ` ${e.message}`);
    }
  });
}

main();
