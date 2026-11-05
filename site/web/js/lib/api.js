// Calls to the site's API (/api/*, Cloud Functions behind Firebase Hosting).

export class ApiError extends Error {
  constructor(message, code, status) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

export async function api(path, { method = 'GET', body, user } = {}) {
  const headers = { Accept: 'application/json' };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (user) headers.Authorization = `Bearer ${await user.getIdToken()}`;
  let res;
  try {
    res = await fetch(`/api${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  } catch {
    throw new ApiError('인터넷 연결을 확인해 주세요.', 'NETWORK', 0);
  }
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new ApiError(data.error?.message ?? `요청이 실패했습니다 (${res.status})`, data.error?.code, res.status);
  return data;
}
