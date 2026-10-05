<script>
  import { onMount } from 'svelte'
  import { api, STATUS_TEXT, fmtTime } from './api.js'
  import Photo from './Photo.svelte'

  let events = []
  let details = {}   // eventId -> detail
  let error = ''
  let notice = ''
  let plateInputs = {}

  async function load() {
    events = await api('/api/events')
    details = {}
  }

  onMount(load)

  async function toggleDetail(ev) {
    if (details[ev.id]) { details = { ...details, [ev.id]: null }; return }
    const d = await api(`/api/events/${ev.id}`)
    details = { ...details, [ev.id]: d }
    if (d.status === 'pending_verify' && !plateInputs[ev.id]) {
      plateInputs[ev.id] = d.plate_reported
    }
  }

  async function verify(ev) {
    error = ''; notice = ''
    try {
      await api(`/api/events/${ev.id}/verify`, {
        method: 'POST',
        body: { confirmed_plate: plateInputs[ev.id] || '' }
      })
      notice = `事件 #${ev.id} 已核实，可通知车主`
      await load()
    } catch (e) { error = e.message }
  }

  async function reject(ev) {
    const reason = prompt('驳回原因：', '证据不足')
    if (reason === null) return
    error = ''; notice = ''
    try {
      await api(`/api/events/${ev.id}/reject`, { method: 'POST', body: { reason } })
      notice = `事件 #${ev.id} 已驳回`
      await load()
    } catch (e) { error = e.message }
  }

  async function notifyOwner(ev) {
    error = ''; notice = ''
    try {
      const res = await api(`/api/events/${ev.id}/notify`, { method: 'POST', body: {} })
      notice = `事件 #${ev.id} 短信已发送至 ${res.phone}（模拟）。注意：发送成功不代表车辆已离开，仍须复查后才能结案`
      await load()
    } catch (e) { error = e.message }
  }

  async function close(ev) {
    error = ''; notice = ''
    try {
      await api(`/api/events/${ev.id}/close`, { method: 'POST', body: {} })
      notice = `事件 #${ev.id} 已结案`
      await load()
    } catch (e) { error = e.message }
  }

  $: pendingVerify = events.filter((e) => e.status === 'pending_verify')
  $: toNotify = events.filter((e) => e.status === 'verified')
  $: notified = events.filter((e) => e.status === 'notified')
  $: finished = events.filter((e) => e.status === 'closed' || e.status === 'rejected')
</script>

{#if error}<div class="card error">{error}</div>{/if}
{#if notice}<div class="card success">{notice}</div>{/if}

<section>
  <h2>待核实 {#if pendingVerify.length}<span class="count">{pendingVerify.length}</span>{/if}</h2>
  {#if pendingVerify.length === 0}<div class="card muted">暂无待核实事件</div>{/if}
  {#each pendingVerify as ev (ev.id)}
    <div class="card">
      <div class="row" style="justify-content:space-between">
        <strong>车位 {ev.spot_number}</strong>
        <span class="badge {ev.status}">{STATUS_TEXT[ev.status]}</span>
      </div>
      <p class="muted">报案车牌 {ev.plate_reported} · 举报 {ev.report_count} 次 · {fmtTime(ev.created_at)}</p>
      {#if ev.needs_manual_review}<p><span class="badge warn">照片不清晰，须人工核实车牌</span></p>{/if}
      <button class="ghost" style="margin-top:8px" on:click={() => toggleDetail(ev)}>
        {details[ev.id] ? '收起证据' : '查看证据并核实'}
      </button>
      {#if details[ev.id]}
        {@const d = details[ev.id]}
        {#each d.reports as r (r.id)}
          <div class="report">
            <Photo reportId={r.id} />
            <p class="muted" style="margin-top:6px">
              {r.reporter} · {fmtTime(r.occurred_at)} · 报案车牌 {r.plate}<br />
              {#if r.needs_manual_review}
                <span class="badge warn">识别置信度低（{Math.round((r.recognition_confidence || 0) * 100)}%），识别结果不可用，请人工核对照片</span>
              {:else}
                辅助识别：{r.recognized_plate || '无'}（{Math.round((r.recognition_confidence || 0) * 100)}%），仅供参考
              {/if}
            </p>
          </div>
        {/each}
        <label>人工确认车牌 *（识别结果仅作辅助，不能直接采用）</label>
        <input bind:value={plateInputs[ev.id]} placeholder="核实后的车牌号" />
        <div class="row" style="margin-top:10px">
          <button class="ok" style="flex:1" on:click={() => verify(ev)}>核实通过</button>
          <button class="danger" style="flex:1" on:click={() => reject(ev)}>驳回</button>
        </div>
      {/if}
    </div>
  {/each}
</section>

<section>
  <h2>待通知车主</h2>
  {#if toNotify.length === 0}<div class="card muted">暂无待通知事件</div>{/if}
  {#each toNotify as ev (ev.id)}
    <div class="card">
      <div class="row" style="justify-content:space-between">
        <strong>车位 {ev.spot_number}</strong>
        <span class="badge verified">待通知</span>
      </div>
      <p class="muted">确认车牌 {ev.plate_confirmed} · {fmtTime(ev.created_at)}</p>
      <button style="width:100%;margin-top:8px" on:click={() => notifyOwner(ev)}>发送短信通知车主</button>
    </div>
  {/each}
</section>

<section>
  <h2>已通知 · 待复查</h2>
  {#if notified.length === 0}<div class="card muted">暂无</div>{/if}
  {#each notified as ev (ev.id)}
    <div class="card">
      <div class="row" style="justify-content:space-between">
        <strong>车位 {ev.spot_number}</strong>
        {#if ev.pending_recheck}
          <span class="badge notified">待复查</span>
        {:else}
          <span class="badge closed">复查已确认离开</span>
        {/if}
      </div>
      <p class="muted">
        车牌 {ev.plate_confirmed} · 已通知 {ev.notified_count} 次 · 最近通知 {fmtTime(ev.last_notified_at)}
      </p>
      <button class="ghost" style="margin-top:8px" on:click={() => toggleDetail(ev)}>
        {details[ev.id] ? '收起记录' : '查看通知/复查记录'}
      </button>
      {#if details[ev.id]}
        {@const d = details[ev.id]}
        {#each d.notifications as n (n.id)}
          <p class="muted">📩 {fmtTime(n.created_at)} 发送至 {n.phone}（{n.status === 'sent' ? '已送达网关' : n.status}）</p>
        {/each}
        {#each d.rechecks as c (c.id)}
          <p class="muted">🔍 {fmtTime(c.created_at)} {c.inspector}：{c.result === 'left' ? '车辆已离开' : '仍占用'}{c.note ? ` · ${c.note}` : ''}</p>
        {/each}
        {#if d.pending_recheck}
          <p class="error">短信已发送但车辆未确认离开，事件保持待处理，不能结案</p>
        {/if}
      {/if}
      <div class="row" style="margin-top:10px">
        <button class="ghost" style="flex:1" on:click={() => notifyOwner(ev)}>再次通知</button>
        <button class="ok" style="flex:1" disabled={!ev.can_close} on:click={() => close(ev)}>
          {ev.can_close ? '结案' : '结案（须先复查确认离开）'}
        </button>
      </div>
    </div>
  {/each}
</section>

<section>
  <h2>已结束</h2>
  {#if finished.length === 0}<div class="card muted">暂无</div>{/if}
  {#each finished as ev (ev.id)}
    <div class="card done">
      <div class="row" style="justify-content:space-between">
        <strong>车位 {ev.spot_number}</strong>
        <span class="badge {ev.status}">{STATUS_TEXT[ev.status]}</span>
      </div>
      <p class="muted">车牌 {ev.plate_confirmed || ev.plate_reported} · 举报 {ev.report_count} 次 · {fmtTime(ev.created_at)}</p>
    </div>
  {/each}
</section>

<style>
  h2 { font-size: 16px; margin: 18px 2px 10px; }
  .count { background: #d64545; color: #fff; border-radius: 999px; padding: 1px 9px; font-size: 13px; }
  .report { border-top: 1px solid #eef1f4; padding-top: 10px; margin-top: 10px; }
  .done { opacity: 0.75; }
</style>
