// 简单的 API 封装:令牌存 localStorage,所有请求带 Bearer。
const TOKEN_KEY = 'ev-token';

export function getToken() {
  return localStorage.getItem(TOKEN_KEY);
}
export function setAuth(token, user) {
  localStorage.setItem(TOKEN_KEY, token);
  localStorage.setItem('ev-user', JSON.stringify(user));
}
export function getUser() {
  try {
    return JSON.parse(localStorage.getItem('ev-user') || 'null');
  } catch {
    return null;
  }
}
export function logout() {
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem('ev-user');
}

async function request(method, path, body) {
  const headers = {};
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  if (!(body instanceof FormData)) headers['Content-Type'] = 'application/json';

  const res = await fetch(`/api${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : body instanceof FormData ? body : JSON.stringify(body)
  });

  const text = await res.text();
  const data = text ? JSON.parse(text) : {};
  if (!res.ok) {
    const err = new Error(data.error || `请求失败 (${res.status})`);
    err.status = res.status;
    throw err;
  }
  return data;
}

export const api = {
  login: (username) => request('POST', '/login', { username }),
  me: () => request('GET', '/me'),
  events: () => request('GET', '/events'),
  event: (id) => request('GET', `/events/${id}`),
  verify: (id, payload) => request('POST', `/events/${id}/verify`, payload),
  notify: (id) => request('POST', `/events/${id}/notify`, {}),
  submitReport: (form) => request('POST', '/reports', form),
  submitRecheck: (id, form) => request('POST', `/events/${id}/rechecks`, form)
};

// 照片受权限保护,不能直接 <img src>,先以鉴权请求取 Blob 再渲染。
export async function photoUrl(filename) {
  const res = await fetch(`/api/photos/${encodeURIComponent(filename)}`, {
    headers: { Authorization: `Bearer ${getToken()}` }
  });
  if (!res.ok) throw new Error('照片无权访问或已丢失');
  return URL.createObjectURL(await res.blob());
}

export const STATUS_LABEL = {
  pending_review: '待核实',
  verified: '已核实 · 待通知',
  pending_recheck: '待复查',
  closed: '已关闭(车辆已离开)'
};
