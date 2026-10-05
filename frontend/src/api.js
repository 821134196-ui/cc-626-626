const TOKEN_KEY = 'pe_token'

export function getToken() { return localStorage.getItem(TOKEN_KEY) }
export function setToken(t) { localStorage.setItem(TOKEN_KEY, t) }
export function clearToken() { localStorage.removeItem(TOKEN_KEY) }

export async function api(path, { method = 'GET', body, form } = {}) {
  const headers = {}
  const token = getToken()
  if (token) headers['Authorization'] = 'Bearer ' + token
  let payload
  if (form) {
    payload = form
  } else if (body !== undefined) {
    headers['Content-Type'] = 'application/json'
    payload = JSON.stringify(body)
  }
  const res = await fetch(path, { method, headers, body: payload })
  if (!res.ok) {
    let msg = `请求失败(${res.status})`
    try { msg = (await res.json()).error || msg } catch { /* ignore */ }
    throw new Error(msg)
  }
  return res.json()
}

/** 照片接口需要登录态，<img> 无法带请求头，故先取 blob 再生成本地 URL */
export async function fetchPhotoUrl(reportId) {
  const res = await fetch(`/api/reports/${reportId}/photo`, {
    headers: { Authorization: 'Bearer ' + getToken() }
  })
  if (!res.ok) throw new Error('照片加载失败')
  return URL.createObjectURL(await res.blob())
}

export const STATUS_TEXT = {
  pending_verify: '待核实',
  verified: '待通知',
  notified: '待复查',
  closed: '已结案',
  rejected: '已驳回'
}

export function fmtTime(iso) {
  if (!iso) return '-'
  const d = new Date(iso)
  if (isNaN(d)) return iso
  return d.toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })
}
