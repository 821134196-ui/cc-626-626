<script>
  import { onMount } from 'svelte'
  import { api, getToken, clearToken } from './api.js'
  import Login from './Login.svelte'
  import Inspector from './Inspector.svelte'
  import Officer from './Officer.svelte'

  let user = null
  let loading = true

  onMount(async () => {
    if (getToken()) {
      try { user = await api('/api/me') } catch { clearToken() }
    }
    loading = false
  })

  function onLogin(e) { user = e.detail }
  function logout() { clearToken(); user = null }
</script>

{#if loading}
  <div class="center">加载中…</div>
{:else if !user}
  <Login on:login={onLogin} />
{:else}
  <div class="page">
    <header>
      <div>
        <h1>新能源车位占用举证</h1>
        <span class="who">{user.name} · {user.role === 'officer' ? '值班员' : '巡查员'}</span>
      </div>
      <button class="ghost" on:click={logout}>退出</button>
    </header>
    {#if user.role === 'officer'}
      <Officer />
    {:else}
      <Inspector />
    {/if}
  </div>
{/if}

<style>
  :global(*) { box-sizing: border-box; margin: 0; padding: 0; }
  :global(body) {
    font-family: -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif;
    background: #f2f4f7; color: #1f2933; font-size: 15px;
  }
  .center { text-align: center; padding: 4rem; color: #7b8794; }
  .page { max-width: 560px; margin: 0 auto; padding: 0 12px 60px; }
  header {
    display: flex; justify-content: space-between; align-items: center;
    padding: 14px 4px; position: sticky; top: 0; background: #f2f4f7; z-index: 10;
  }
  h1 { font-size: 18px; }
  .who { font-size: 12px; color: #7b8794; }
  :global(button) {
    border: none; border-radius: 10px; padding: 10px 16px; font-size: 15px;
    background: #0b6bcb; color: #fff; cursor: pointer; min-height: 42px;
  }
  :global(button:disabled) { background: #b6c2cf; cursor: not-allowed; }
  :global(button.ghost) { background: transparent; color: #52606d; border: 1px solid #cbd2d9; min-height: 34px; padding: 6px 12px; font-size: 13px; }
  :global(button.danger) { background: #d64545; }
  :global(button.ok) { background: #1f9d63; }
  :global(.card) {
    background: #fff; border-radius: 12px; padding: 14px; margin-bottom: 12px;
    box-shadow: 0 1px 3px rgba(15, 23, 42, 0.08);
  }
  :global(.row) { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  :global(.muted) { color: #7b8794; font-size: 13px; }
  :global(input), :global(textarea) {
    width: 100%; padding: 10px; border: 1px solid #cbd2d9; border-radius: 8px;
    font-size: 15px; background: #fff;
  }
  :global(label) { display: block; font-size: 13px; color: #52606d; margin: 10px 0 4px; }
  :global(.error) { color: #d64545; font-size: 13px; margin-top: 8px; }
  :global(.success) { color: #1f9d63; font-size: 13px; margin-top: 8px; }
  :global(.badge) {
    display: inline-block; padding: 2px 8px; border-radius: 999px;
    font-size: 12px; background: #e4e7eb; color: #3e4c59;
  }
  :global(.badge.pending_verify) { background: #fff3c4; color: #8d6b00; }
  :global(.badge.verified) { background: #c8f7c5; color: #1c6b2f; }
  :global(.badge.notified) { background: #ffd8a8; color: #8a4b00; }
  :global(.badge.closed) { background: #d3f9d8; color: #2b8a3e; }
  :global(.badge.rejected) { background: #e4e7eb; color: #7b8794; }
  :global(.badge.warn) { background: #ffe3e3; color: #c92a2a; }
</style>
