// What the site sells. The server is the only source of truth for prices: the page shows what
// /api/catalog returns, and an order's amount is always taken from here, never from the browser.
//
// 가격 · 구성을 바꾸려면 이 파일만 고치고 함수를 다시 배포하세요 (README 참고).

export const CURRENCY = 'KRW';

// The seller is registered as a VAT-exempt business (부가가치세 면세사업자): the whole amount is
// reported as tax-free (Toss Payments `taxFreeAmount`; ignored for taxable merchants). Set to
// false if the business becomes a taxable one (과세사업자).
export const TAX_FREE = true;

export const PRODUCTS = [
  {
    id: 'license',
    name: 'LOUPE 라이선스',
    // Shown in the payment window and on the receipt (max 100 characters).
    orderName: 'LOUPE 라이선스 (영구)',
    price: 89000,
    priceNote: '한 번 결제 · 구독 없음',
    maxDevices: 3,
    updatesMonths: 12,
    features: [
      '가져오기 · 정리 · 셀렉 · 작품 폴더, 모든 기능',
      '셀렉 도우미 — B컷 · A컷 후보, 내 기준과 취향 학습',
      '내 컴퓨터 3대까지 (맥 · 윈도우 · 리눅스)',
      '1년 동안 새 버전 무료 — 그 뒤에도 쓰던 버전은 계속',
    ],
  },
];

export const TRIAL = {
  title: '먼저 써 보세요',
  text: '다운로드는 무료예요. 내 사진으로 충분히 써 본 뒤에 결정하세요.',
};

export function product(id) {
  return PRODUCTS.find((p) => p.id === id) ?? null;
}

/** The public catalog (prices, features, how to pay). */
export function publicCatalog(payments) {
  return { currency: CURRENCY, products: PRODUCTS, trial: TRIAL, payments };
}
