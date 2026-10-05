<script>
  import { onMount } from 'svelte';
  import { api, setAuth, getUser, logout, STATUS_LABEL } from './api.js';
  import ReportForm from './lib/ReportForm.svelte';
  import EventDetail from './lib/EventDetail.svelte';

  let user = getUser();
  let events = [];
  let selected = null;
  let loading = false;
  let busy = false;
  let toast = '';
  let loginName = '';
  let loginErr = '';
  let showForm = false;

  let toastTimer;
  function flash(msg) {
    toast = msg;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = ''), 4000);
  }

  async function doLogin() {
    loginErr = '';
    try {
      const r = await api.login(loginName.trim());
      setAuth(r.token, r.user);
      user = r.user;
      await refresh();
    } catch (e) {
      loginErr = e.message;
    }
  }

  function doLogout() {
    logout();
    user = null;
    selected = null;
    events = [];
  }

  async function refresh() {
    loading = true;
    try {
      const r = await api.events();
      events = r.events;
    } catch (e) {
      flash(e.message);
    } finally {
      loading = false;
    }
  }

  async function openEvent(id) {
    try {
      selected = await api.event(id);
    } catch (e) {
      flash(e.status === 403 ? '无权查看该事件资料' : e.message);
    }
  }

  async function onReport(form) {
    busy = true;
    try {
      const r = await api.submitReport(form);
      flash(
        r.merged
          ? `已归并到车位 ${r.event.spot_no} 的进行中事件 #${r.event.id}`
          : `已立事件 #${r.event.id},等待值班员核实`
      );
      if (r.recognition.needs_manual) flash('照片识别不清晰,已标记转人工核实');
      showForm = false;
      await refresh();
    } catch (e) {
      flash(e.message);
    } finally {
      busy = false;
    }
  }

  async function onVerify(e) {
    busy = true;
    try {
      await api.verify(selected.event.id, e.detail);
      flash('已人工核实车牌,可通知车主');
      selected = await api.event(selected.event.id);
      await refresh();
    } catch (err) {
      flash(err.message);
    } finally {
      busy = false;
    }
  }

  async function onNotify() {
    busy = true;
    try {
      const r = await api.notify(selected.event.id);
      flash(`短信已发送(回执 ${r.sms.provider_msg_id}),事件保持待复查`);
      selected = await api.event(selected.event.id);
      await refresh();
    } catch (err) {
      flash(err.message);
    } finally {
      busy = false;
    }
  }

  async function onRecheck(e) {
    busy = true;
    try {
      const form = new FormData();
      form.set('vehicle_left', e.detail.vehicle_left);
      if (e.detail.note) form.set('note', e.detail.note);
      const r = await api.submitRecheck(selected.event.id, form);
      flash(r.message);
      selected = await api.event(selected.event.id);
      await refresh();
    } catch (err) {
      flash(err.message);
    } finally {
      busy = false;
    }
  }

  // 待复查优先(后端已排序,前端再分组展示)
  $: pendingRecheck = events.filter((e) => e.status === 'pending_recheck');
  $: others = events.filter((e) => e.status !== 'pending_recheck');

  onMount(() => {
    if (user) refresh();
  });
</script>

{#if !user}
  <main class="login">
    <h1>新能源车位<br />占用举证系统</h1>
    <p class="sub">公共停车场 · 专用车位管理</p>
    <form on:submit|preventDefault={doLogin}>
      <input placeholder="输入账号登录" bind:value={loginName} autocomplete="username" />
      <button class="primary" type="submit">登录</button>
      {#if loginErr}<p class="err">{loginErr}</p>{/if}
    </form>
    <div class="accounts">
      <p>演示账号(无需密码):</p>
      <button class="acct" on:click={() => (loginName = 'inspector1')}>inspector1 · 巡查员甲</button>
      <button class="acct" on:click={() => (loginName = 'inspector2')}>inspector2 · 巡查员乙</button>
      <button class="acct" on:click={() => (loginName = 'duty1')}>duty1 · 值班员甲</button>
    </div>
  </main>
{:else if selected}
  <main class="page">
    {#key selected.event.id}
      <EventDetail
        {user}
        detail={selected}
        {busy}
        on:back={() => (selected = null)}
        on:verify={onVerify}
        on:notify={onNotify}
        on:recheck={onRecheck}
      />
    {/key}
  </main>
{:else}
  <main class="page">
    <header class="topbar">
      <div>
        <strong>{user.display_name}</strong>
        <span class="role">{user.role === 'duty' ? '值班员' : '巡查员'}</span>
      </div>
      <button class="logout" on:click={doLogout}>退出</button>
    </header>

    {#if user.role === 'inspector'}
      <button class="toggle" on:click={() => (showForm = !showForm)}>
        {showForm ? '收起举报表单' : '＋ 登记占用举报'}
      </button>
      {#if showForm}<ReportForm {busy} on:submit={onReport} />{/if}

      <section>
        <h2 class="urgent">待复查车位 · {pendingRecheck.length}</h2>
        <p class="sub">已通知车主,需到场确认车辆确实离开后才能关闭事件</p>
        {#if pendingRecheck.length === 0}
          <p class="empty">暂无待复查车位</p>
        {/if}
        {#each pendingRecheck as e (e.id)}
          <button class="card urgent-card" on:click={() => openEvent(e.id)}>
            <span class="badge s-{e.status}">{STATUS_LABEL[e.status]}</span>
            <span class="spot">{e.spot_no}</span>
            <span class="plate">{e.confirmed_plate ?? ''}</span>
            <span class="meta">通知 {e.notice_count} 次 · 举报 {e.report_count} 次</span>
          </button>
        {/each}
      </section>

      <section>
        <h2>其他事件</h2>
        {#each others as e (e.id)}
          <button class="card" on:click={() => openEvent(e.id)}>
            <span class="badge s-{e.status}">{STATUS_LABEL[e.status]}</span>
            <span class="spot">{e.spot_no}</span>
            <span class="plate">{e.confirmed_plate ?? '车牌待值班员核实'}</span>
            <span class="meta">{e.last_report_at}</span>
          </button>
        {/each}
        {#if others.length === 0}<p class="empty">暂无其他事件</p>{/if}
      </section>
    {:else}
      <section>
        <h2>全部事件 · 待复查优先</h2>
        <p class="sub">核实车牌后通知车主;结案只能由巡查员现场复查确认</p>
        {#if loading}<p class="empty">加载中…</p>{/if}
        {#each [...pendingRecheck, ...others] as e (e.id)}
          <button class="card" on:click={() => openEvent(e.id)}>
            <span class="badge s-{e.status}">{STATUS_LABEL[e.status]}</span>
            <span class="spot">{e.spot_no}</span>
            <span class="plate">{e.confirmed_plate ?? '车牌未核实'}</span>
            <span class="meta">举报 {e.report_count} 次 · 通知 {e.notice_count} 次</span>
          </button>
        {/each}
        {#if events.length === 0 && !loading}<p class="empty">暂无事件</p>{/if}
      </section>
    {/if}
  </main>
{/if}

{#if toast}<div class="toast" role="status">{toast}</div>{/if}

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, "PingFang SC", "Microsoft YaHei", system-ui, sans-serif;
    background: #eef1f5;
    color: #1f2937;
  }
  main.login {
    max-width: 420px;
    margin: 0 auto;
    padding: 48px 20px;
    text-align: center;
  }
  .login h1 { font-size: 26px; line-height: 1.35; margin: 0; }
  .login .sub, .sub { color: #64748b; font-size: 13px; }
  .login input {
    width: 100%; box-sizing: border-box; padding: 13px; margin: 20px 0 10px;
    border: 1px solid #cbd5e1; border-radius: 10px; font-size: 16px;
  }
  .primary {
    width: 100%; padding: 13px; border: none; border-radius: 10px;
    background: #2563eb; color: #fff; font-size: 16px; font-weight: 700;
  }
  .err { color: #b91c1c; font-size: 13px; }
  .accounts { margin-top: 28px; text-align: left; }
  .accounts p { color: #64748b; font-size: 13px; margin-bottom: 6px; }
  button.acct {
    display: block; width: 100%; text-align: left; padding: 10px 12px; margin: 6px 0;
    background: #fff; border: 1px solid #dbe2ea; border-radius: 8px; font-size: 14px;
  }
  main.page {
    max-width: 560px; margin: 0 auto; padding: 12px 14px 40px;
  }
  .topbar {
    display: flex; justify-content: space-between; align-items: center;
    padding: 6px 2px 12px;
  }
  .role {
    margin-left: 8px; font-size: 12px; background: #e0e7ff; color: #3730a3;
    padding: 2px 8px; border-radius: 999px;
  }
  .logout {
    border: 1px solid #cbd5e1; background: #fff; border-radius: 8px;
    padding: 6px 12px; font-size: 13px;
  }
  .toggle {
    width: 100%; padding: 12px; margin-bottom: 12px; border: none;
    border-radius: 10px; background: #2563eb; color: #fff;
    font-size: 15px; font-weight: 700;
  }
  h2 { font-size: 16px; margin: 18px 0 4px; }
  h2.urgent { color: #be123c; }
  .empty { color: #94a3b8; font-size: 13px; text-align: center; padding: 12px; }
  .card {
    display: grid;
    grid-template-columns: auto 1fr auto;
    grid-template-rows: auto auto;
    gap: 2px 10px;
    width: 100%; text-align: left; margin: 8px 0;
    background: #fff; border: 1px solid #e2e7ee; border-radius: 12px;
    padding: 12px; cursor: pointer; font-size: 14px;
  }
  .urgent-card { border-color: #fecdd3; background: #fff5f6; }
  .badge {
    grid-row: 1 / 3;
    align-self: center;
    font-size: 12px; padding: 4px 10px; border-radius: 999px; color: #fff;
    white-space: nowrap;
  }
  .s-pending_review { background: #b45309; }
  .s-verified { background: #0369a1; }
  .s-pending_recheck { background: #be123c; }
  .s-closed { background: #15803d; }
  .spot { font-size: 17px; font-weight: 700; }
  .plate { font-size: 14px; color: #334155; }
  .meta { grid-column: 2 / 4; font-size: 12px; color: #64748b; }
  .toast {
    position: fixed; left: 50%; bottom: 24px; transform: translateX(-50%);
    background: #111827; color: #fff; padding: 10px 16px; border-radius: 10px;
    font-size: 13px; max-width: 90vw; z-index: 100;
  }
</style>
