<script>
  import { createEventDispatcher } from 'svelte';
  import Photo from './Photo.svelte';
  import { STATUS_LABEL } from '../api.js';

  export let detail;
  export let user;
  export let busy = false;

  const dispatch = createEventDispatcher();

  let plateInput = '';
  let verifyNote = '';
  let recheckNote = '';
  $: plateInput = plateInput || latestRecognition(detail) || '';

  function latestRecognition(d) {
    const rs = d?.reports ?? [];
    for (let i = rs.length - 1; i >= 0; i--) {
      if (rs[i].suggested_plate) return rs[i].suggested_plate;
    }
    return '';
  }
  function anyManual(d) {
    return (d?.reports ?? []).some((r) => r.needs_manual);
  }
</script>

<div class="detail">
  <header>
    <button class="back" on:click={() => dispatch('back')}>← 返回列表</button>
    <h2>车位 {detail.event.spot_no}</h2>
    <span class="badge s-{detail.event.status}">{STATUS_LABEL[detail.event.status]}</span>
  </header>

  <div class="meta">
    <div>确认车牌:<b>{detail.event.confirmed_plate ?? '尚未人工核实'}</b></div>
    <div>举报次数:{detail.event.report_count}(归并同一占用事件)</div>
    <div>通知次数:{detail.event.notice_count}</div>
    <div>首次举报:{detail.event.first_reported_at}</div>
    <div>最近举报:{detail.event.last_report_at}</div>
    {#if detail.event.closed_at}<div>关闭时间:{detail.event.closed_at}</div>{/if}
  </div>

  <!-- 值班员操作区 -->
  {#if user.role === 'duty'}
    <section class="panel">
      <h3>① 核实车牌(人工)</h3>
      {#if detail.verification}
        <p class="ok">
          已由 {detail.verification.duty_name} 核实确认:
          <b>{detail.verification.confirmed_plate}</b>({detail.verification.created_at})
        </p>
      {:else}
        {#if anyManual(detail)}
          <p class="warn">⚠ 存在识别不清晰的照片,车牌识别结果不可采信,请对照照片人工核实后填写。</p>
        {:else}
          <p class="hint">识别建议仅作辅助预填,请对照照片核对后确认,不得直接按识别结果通知。</p>
        {/if}
        <div class="row">
          <input placeholder="人工确认车牌,如 沪A12345" bind:value={plateInput}
                 disabled={detail.event.status !== 'pending_review'} />
          <button class="primary" disabled={busy || detail.event.status !== 'pending_review'}
                  on:click={() => dispatch('verify', { plate: plateInput, note: verifyNote })}>
            核实通过
          </button>
        </div>
        <input class="note" placeholder="核实备注(可选)" bind:value={verifyNote}
               disabled={detail.event.status !== 'pending_review'} />
      {/if}
    </section>

    <section class="panel">
      <h3>② 通知车主(模拟短信)</h3>
      {#if !detail.verification}
        <p class="hint">必须先完成人工核实,才能通知车主。</p>
      {/if}
      <button class="primary" disabled={busy || !detail.verification ||
        (detail.event.status !== 'verified' && detail.event.status !== 'pending_recheck')}
        on:click={() => dispatch('notify')}>
        发送挪车短信
      </button>
      {#if detail.event.status === 'pending_recheck'}
        <p class="warn">短信已发出,但事件仍为「待复查」——发送成功不代表车辆离开,须等待巡查员现场确认。可再次催办。</p>
      {/if}
    </section>

    <section class="panel readonly">
      <h3>③ 关闭事件</h3>
      <p>值班员无权直接结案。只有巡查员复查确认车辆离开后,系统才会关闭事件。</p>
    </section>
  {/if}

  <!-- 巡查员操作区 -->
  {#if user.role === 'inspector'}
    <section class="panel">
      <h3>现场复查</h3>
      {#if detail.event.status === 'pending_recheck'}
        <p class="hint">请到达车位 <b>{detail.event.spot_no}</b> 现场核实车辆是否已经离开,可补拍照片。</p>
        <input class="note" placeholder="复查备注(可选)" bind:value={recheckNote} />
        <div class="row">
          <button class="danger" disabled={busy}
                  on:click={() => dispatch('recheck', { vehicle_left: false, note: recheckNote })}>
            车辆仍占用
          </button>
          <button class="primary" disabled={busy}
                  on:click={() => dispatch('recheck', { vehicle_left: true, note: recheckNote })}>
            确认车辆已离开
          </button>
        </div>
        <p class="warn">只有「确认车辆已离开」才会关闭事件;短信发送成功不能代替复查。</p>
      {:else if detail.event.status === 'closed'}
        <p class="ok">✓ 已复查确认车辆离开,事件关闭。车辆若再次驶入被举报,将自动重新立案。</p>
      {:else}
        <p class="hint">值班员尚未发出通知,暂不能复查。</p>
      {/if}
    </section>
  {/if}

  <!-- 时间线:举报与照片 -->
  <section class="panel">
    <h3>举证记录({detail.reports.length})</h3>
    {#each detail.reports as r}
      <div class="record">
        <Photo filename={r.photo_path} alt={`举报照片 ${r.id}`} />
        <div class="rec">
          <div>巡查员:{r.inspector_name} · {r.created_at}</div>
          <div>登记车牌:{r.plate_entered} · 发生时间:{r.occurred_at}</div>
          {#if r.needs_manual}
            <div class="warn">识别建议「{r.suggested_plate ?? '无'}」(置信度 {(r.confidence*100).toFixed(0)}%)→ 照片不清,转人工核实</div>
          {:else}
            <div class="hint">识别建议:{r.suggested_plate}(置信度 {(r.confidence*100).toFixed(0)}%,仅辅助)</div>
          {/if}
        </div>
      </div>
    {/each}
  </section>

  {#if detail.notifications.length}
    <section class="panel">
      <h3>通知记录({detail.notifications.length})</h3>
      {#each detail.notifications as n}
        <div class="notice">
          <span class="tag sent">{n.status === 'sent' ? '短信已送达网关' : '发送失败'}</span>
          <span>{n.created_at} · 车牌 {n.plate}</span>
          <p>{n.message}</p>
        </div>
      {/each}
    </section>
  {/if}

  {#if detail.rechecks.length}
    <section class="panel">
      <h3>复查记录({detail.rechecks.length})</h3>
      {#each detail.rechecks as rc}
        <div class="record">
          {#if rc.photo_path}<Photo filename={rc.photo_path} alt="复查照片" />{/if}
          <div class="rec">
            <span class="tag {rc.vehicle_left ? 'left' : 'stuck'}">
              {rc.vehicle_left ? '✓ 车辆已离开' : '✗ 车辆仍占用'}
            </span>
            <div>{rc.inspector_name} · {rc.created_at}</div>
            {#if rc.note}<div>备注:{rc.note}</div>{/if}
          </div>
        </div>
      {/each}
    </section>
  {/if}
</div>

<style>
  .detail header { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  h2 { margin: 0; font-size: 20px; }
  .back {
    border: none; background: none; color: #2563eb; font-size: 15px; padding: 6px 0;
  }
  .badge {
    font-size: 13px; padding: 3px 10px; border-radius: 999px; color: #fff;
  }
  .s-pending_review { background: #b45309; }
  .s-verified { background: #0369a1; }
  .s-pending_recheck { background: #be123c; }
  .s-closed { background: #15803d; }
  .meta {
    background: #f3f5f8; border-radius: 10px; padding: 10px 12px; margin: 10px 0;
    font-size: 14px; line-height: 1.9; color: #374151;
  }
  .panel {
    border: 1px solid #e2e7ee; border-radius: 12px; padding: 12px; margin: 10px 0;
  }
  .panel.readonly { background: #f8fafc; color: #6b7280; }
  .panel h3 { margin: 0 0 8px; font-size: 15px; }
  .row { display: flex; gap: 8px; margin-top: 8px; }
  input {
    width: 100%; padding: 10px; border: 1px solid #cbd5e1; border-radius: 8px;
    font-size: 15px; box-sizing: border-box;
  }
  input.note { margin-top: 8px; }
  button {
    padding: 10px 14px; border-radius: 8px; border: none; font-size: 15px;
    font-weight: 600; color: #fff; background: #64748b;
  }
  button:disabled { opacity: .45; }
  button.primary { background: #2563eb; flex: 1; }
  button.danger { background: #b91c1c; flex: 1; }
  .hint { color: #64748b; font-size: 13px; margin: 4px 0; }
  .warn { color: #b45309; font-size: 13px; margin: 6px 0; font-weight: 600; }
  .ok { color: #15803d; font-size: 14px; margin: 4px 0; }
  .record { display: flex; gap: 10px; padding: 8px 0; border-top: 1px dashed #e2e7ee; }
  .record:first-child { border-top: none; }
  .rec { font-size: 13px; line-height: 1.7; color: #374151; min-width: 0; }
  .notice { border-top: 1px dashed #e2e7ee; padding: 8px 0; font-size: 13px; }
  .notice:first-child { border-top: none; }
  .notice p { margin: 4px 0 0; color: #4b5563; }
  .tag {
    display: inline-block; font-size: 12px; padding: 2px 8px; border-radius: 999px;
    color: #fff; margin-right: 6px;
  }
  .tag.sent { background: #0369a1; }
  .tag.left { background: #15803d; }
  .tag.stuck { background: #b91c1c; }
</style>
