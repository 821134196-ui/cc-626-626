<script>
  import { onMount } from 'svelte'
  import { api, STATUS_TEXT, fmtTime } from './api.js'

  let events = []
  let error = ''
  let notice = ''

  // 新举报表单
  let spot = ''
  let plate = ''
  let occurredAt = new Date().toISOString().slice(0, 16)
  let photo = null
  let submitting = false

  // 复查备注
  let notes = {}

  async function load() {
    events = await api('/api/events')
  }

  onMount(load)

  $: recheckList = events.filter((e) => e.status === 'notified')
  $: mine = events.filter((e) => e.status !== 'notified')

  async function submitReport() {
    error = ''; notice = ''
    if (!spot || !plate || !photo) { error = '车位号、车牌、现场照片均为必填'; return }
    submitting = true
    try {
      const form = new FormData()
      form.append('spot_number', spot)
      form.append('plate', plate)
      form.append('occurred_at', occurredAt)
      form.append('photo', photo)
      const res = await api('/api/reports', { method: 'POST', form })
      const rec = res.recognition
      if (res.merged) {
        notice = `该车位已有未结案事件 #${res.event_id}，本次举报已归并（不重复立案）`
      } else {
        notice = `已立案，事件 #${res.event_id}，等待值班员核实`
      }
      if (rec.needs_manual_review) {
        notice += '；照片不清晰，已标记转人工核实'
      }
      spot = ''; plate = ''; photo = null
      document.getElementById('photo-input').value = ''
      await load()
    } catch (e) { error = e.message } finally { submitting = false }
  }

  async function doRecheck(ev, result) {
    error = ''; notice = ''
    try {
      await api(`/api/events/${ev.id}/rechecks`, {
        method: 'POST',
        body: { result, note: notes[ev.id] || null }
      })
      notice = result === 'left'
        ? `事件 #${ev.id} 已登记"车辆离开"，等待值班员结案`
        : `事件 #${ev.id} 已登记"仍占用"，事件保持待处理`
      notes[ev.id] = ''
      await load()
    } catch (e) { error = e.message }
  }
</script>

<!-- 待复查车位优先展示 -->
<section>
  <h2>待复查车位 {#if recheckList.length}<span class="count">{recheckList.length}</span>{/if}</h2>
  {#if recheckList.length === 0}
    <div class="card muted">暂无待复查车位</div>
  {/if}
  {#each recheckList as ev (ev.id)}
    <div class="card urgent">
      <div class="row" style="justify-content:space-between">
        <strong>车位 {ev.spot_number}</strong>
        <span class="badge notified">待复查</span>
      </div>
      <p class="muted">车牌 {ev.plate_confirmed || ev.plate_reported} · 已通知 {ev.notified_count} 次 · 最近通知 {fmtTime(ev.last_notified_at)}</p>
      <label>复查备注（可选）</label>
      <input bind:value={notes[ev.id]} placeholder="现场情况说明" />
      <div class="row" style="margin-top:10px">
        <button class="ok" style="flex:1" on:click={() => doRecheck(ev, 'left')}>车辆已离开</button>
        <button class="danger" style="flex:1" on:click={() => doRecheck(ev, 'still_occupied')}>仍占用</button>
      </div>
    </div>
  {/each}
</section>

<section>
  <h2>提交占用举报</h2>
  <div class="card">
    <label>车位号 *</label>
    <input bind:value={spot} placeholder="如 A-101" />
    <label>车牌号 *</label>
    <input bind:value={plate} placeholder="如 京AD12345" />
    <label>发生时间 *</label>
    <input type="datetime-local" bind:value={occurredAt} />
    <label>现场照片 *</label>
    <input id="photo-input" type="file" accept="image/*" capture="environment"
           on:change={(e) => (photo = e.target.files[0] || null)} />
    <button style="width:100%;margin-top:14px" on:click={submitReport} disabled={submitting}>
      {submitting ? '提交中…' : '提交举报'}
    </button>
    {#if error}<p class="error">{error}</p>{/if}
    {#if notice}<p class="success">{notice}</p>{/if}
  </div>
</section>

<section>
  <h2>相关事件</h2>
  {#if mine.length === 0}
    <div class="card muted">暂无其他事件</div>
  {/if}
  {#each mine as ev (ev.id)}
    <div class="card">
      <div class="row" style="justify-content:space-between">
        <strong>车位 {ev.spot_number}</strong>
        <span class="badge {ev.status}">{STATUS_TEXT[ev.status]}</span>
      </div>
      <p class="muted">
        车牌 {ev.plate_confirmed || ev.plate_reported} · 举报 {ev.report_count} 次 · {fmtTime(ev.created_at)}
        {#if ev.needs_manual_review}<span class="badge warn">需人工核实</span>{/if}
      </p>
    </div>
  {/each}
</section>

<style>
  h2 { font-size: 16px; margin: 18px 2px 10px; }
  .count { background: #d64545; color: #fff; border-radius: 999px; padding: 1px 9px; font-size: 13px; }
  .urgent { border-left: 4px solid #f08c00; }
</style>
