<script>
  import { createEventDispatcher } from 'svelte'
  import { api, setToken } from './api.js'

  const dispatch = createEventDispatcher()
  let username = ''
  let password = ''
  let error = ''
  let busy = false

  async function submit() {
    error = ''
    busy = true
    try {
      const res = await api('/api/login', { method: 'POST', body: { username, password } })
      setToken(res.token)
      dispatch('login', res.user)
    } catch (e) {
      error = e.message
    } finally {
      busy = false
    }
  }
</script>

<div class="login">
  <div class="card">
    <h2>新能源车位占用举证系统</h2>
    <p class="muted">公共停车场专用车位巡查取证</p>
    <label>账号</label>
    <input bind:value={username} placeholder="用户名" autocomplete="username" />
    <label>密码</label>
    <input type="password" bind:value={password} placeholder="密码" autocomplete="current-password"
           on:keydown={(e) => e.key === 'Enter' && submit()} />
    <button style="width:100%;margin-top:16px" on:click={submit} disabled={busy || !username || !password}>
      {busy ? '登录中…' : '登 录'}
    </button>
    {#if error}<p class="error">{error}</p>{/if}
    <p class="muted" style="margin-top:16px;line-height:1.8">
      演示账号 —— 巡查员：xuncha01 / xuncha123<br />
      值班员：zhiban01 / zhiban123
    </p>
  </div>
</div>

<style>
  .login { max-width: 420px; margin: 8vh auto; padding: 0 16px; }
  h2 { font-size: 19px; margin-bottom: 4px; }
</style>
