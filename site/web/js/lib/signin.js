// Sign in with Google, or with a link sent by email (no password).

import {
  GoogleAuthProvider,
  isSignInWithEmailLink,
  sendSignInLinkToEmail,
  signInWithEmailLink,
  signInWithPopup,
  signInWithRedirect,
} from 'firebase/auth';
import { $, esc, toast } from './site.js';

const EMAIL_KEY = 'po.signInEmail';

const GOOGLE_LOGO =
  '<svg viewBox="0 0 48 48" aria-hidden="true"><path fill="#FFC107" d="M43.6 20.5H42V20H24v8h11.3C33.7 32.7 29.2 36 24 36c-6.6 0-12-5.4-12-12s5.4-12 12-12c3.1 0 5.8 1.2 7.9 3.1l5.7-5.7C34 6.1 29.3 4 24 4 12.9 4 4 12.9 4 24s8.9 20 20 20 20-8.9 20-20c0-1.3-.1-2.4-.4-3.5z"/><path fill="#FF3D00" d="m6.3 14.7 6.6 4.8C14.7 15.1 19 12 24 12c3.1 0 5.8 1.2 7.9 3.1l5.7-5.7C34 6.1 29.3 4 24 4 16.3 4 9.7 8.3 6.3 14.7z"/><path fill="#4CAF50" d="M24 44c5.2 0 9.9-2 13.4-5.2l-6.2-5.2A11.9 11.9 0 0 1 24 36c-5.2 0-9.6-3.3-11.3-7.9l-6.5 5C9.5 39.6 16.2 44 24 44z"/><path fill="#1976D2" d="M43.6 20.5H42V20H24v8h11.3a12 12 0 0 1-4.1 5.6l6.2 5.2C37 39.2 44 34 44 24c0-1.3-.1-2.4-.4-3.5z"/></svg>';

function storage(fn) {
  try {
    return fn(localStorage);
  } catch {
    return null;
  }
}

export function renderSignIn(el, auth, { next = location.pathname } = {}) {
  el.innerHTML = `
    <div class="signin">
      <button class="btn btn-google" type="button" data-google>${GOOGLE_LOGO}Google 계정으로 계속하기</button>
      <div class="divider">또는 이메일로</div>
      <form class="field" data-email-form novalidate>
        <label class="sr-only" for="signin-email">이메일</label>
        <input id="signin-email" type="email" name="email" required placeholder="you@example.com" autocomplete="email" inputmode="email">
        <button class="btn btn-soft" type="submit">링크 받기</button>
      </form>
      <p class="small muted" style="margin:0" data-email-help>비밀번호 없이, 메일로 받은 링크를 누르면 로그인돼요.</p>
    </div>`;

  $('[data-google]', el).addEventListener('click', async () => {
    const provider = new GoogleAuthProvider();
    provider.setCustomParameters({ prompt: 'select_account' });
    try {
      await signInWithPopup(auth, provider);
    } catch (e) {
      if (e.code === 'auth/popup-blocked' || e.code === 'auth/operation-not-supported-in-this-environment') {
        await signInWithRedirect(auth, provider);
      } else if (e.code !== 'auth/popup-closed-by-user' && e.code !== 'auth/cancelled-popup-request') {
        toast('로그인하지 못했어요. 다시 시도해 주세요.');
      }
    }
  });

  $('[data-email-form]', el).addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const form = ev.currentTarget;
    const input = form.elements.email;
    const email = input.value.trim();
    if (!email || !input.checkValidity()) {
      input.focus();
      toast('이메일 주소를 확인해 주세요.');
      return;
    }
    const button = form.querySelector('button');
    button.disabled = true;
    try {
      await sendSignInLinkToEmail(auth, email, {
        url: `${location.origin}/account?next=${encodeURIComponent(next)}`,
        handleCodeInApp: true,
      });
      storage((s) => s.setItem(EMAIL_KEY, email));
      $('[data-email-help]', el).innerHTML = `<b>${esc(email)}</b> 로 로그인 링크를 보냈어요. 메일의 링크를 누르면 이어서 진행돼요.`;
    } catch {
      toast('메일을 보내지 못했어요. 주소를 확인해 주세요.');
    } finally {
      button.disabled = false;
    }
  });
}

/** Finish an email-link sign-in if this page was opened from one; returns where to go next. */
export async function completeEmailLink(auth) {
  if (!isSignInWithEmailLink(auth, location.href)) return null;
  let email = storage((s) => s.getItem(EMAIL_KEY));
  if (!email) email = window.prompt('로그인 링크를 받은 이메일 주소를 입력해 주세요.') ?? '';
  await signInWithEmailLink(auth, email.trim(), location.href);
  storage((s) => s.removeItem(EMAIL_KEY));
  const next = new URL(location.href).searchParams.get('next');
  return next && next.startsWith('/') && !next.startsWith('//') ? next : '/account';
}
