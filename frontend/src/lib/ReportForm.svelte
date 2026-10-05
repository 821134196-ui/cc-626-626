<script>
  import { createEventDispatcher } from 'svelte';

  export let busy = false;
  const dispatch = createEventDispatcher();

  let spot_no = '';
  let plate = '';
  let occurredAtLocal = nowLocal();
  let photoFile = null;
  let previewUrl = null;
  let error = '';

  function nowLocal() {
    const d = new Date();
    d.setMinutes(d.getMinutes() - d.getTimezoneOffset());
    return d.toISOString().slice(0, 16);
  }

  function onPick(e) {
    photoFile = e.target.files?.[0] ?? null;
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    previewUrl = photoFile ? URL.createObjectURL(photoFile) : null;
  }

  async function submit() {
    error = '';
    if (!spot_no.trim()) return (error = '请填写车位号');
    if (!plate.trim()) return (error = '请填写车牌号');
    if (!photoFile) return (error = '请拍摄或选择现场照片(举证必需)');

    const occurred = new Date(occurredAtLocal).toISOString();
    const form = new FormData();
    form.set('spot_no', spot_no.trim());
    form.set('plate', plate.trim());
    form.set('occurred_at', occurred);
    form.set('photo', photoFile);
    dispatch('submit', form);
  }

  export function reset() {
    spot_no = '';
    plate = '';
    occurredAtLocal = nowLocal();
    photoFile = null;
    if (previewUrl) URL.revokeObjectURL(previewUrl);
    previewUrl = null;
  }
</script>

<form class="report" on:submit|preventDefault={submit}>
  <h3>登记占用举报</h3>
  <label>新能源车位号
    <input placeholder="如 A-013" bind:value={spot_no} autocomplete="off" />
  </label>
  <label>车牌号(现场登记)
    <input placeholder="如 沪A12345" bind:value={plate} autocomplete="off" />
  </label>
  <label>发生时间
    <input type="datetime-local" bind:value={occurredAtLocal} />
  </label>
  <label class="photo-label">现场照片(必传)
    <input type="file" accept="image/*" capture="environment" on:change={onPick} />
  </label>
  {#if previewUrl}
    <img class="preview" src={previewUrl} alt="现场照片预览" />
  {/if}
  {#if error}<p class="err">{error}</p>{/if}
  <button class="primary" type="submit" disabled={busy}>提交举证</button>
  <p class="hint">同一车位连续占用的重复举报会自动归并到当前事件;若照片模糊,识别将转人工核实。</p>
</form>

<style>
  .report {
    border: 1px solid #e2e7ee;
    border-radius: 12px;
    padding: 12px;
    margin-bottom: 12px;
    background: #fff;
  }
  h3 { margin: 0 0 8px; font-size: 16px; }
  label { display: block; font-size: 13px; color: #475569; margin: 8px 0; }
  input {
    width: 100%; margin-top: 4px; padding: 10px; box-sizing: border-box;
    border: 1px solid #cbd5e1; border-radius: 8px; font-size: 15px;
  }
  .preview { width: 100%; max-height: 220px; object-fit: cover; border-radius: 8px; }
  button.primary {
    width: 100%; padding: 12px; border: none; border-radius: 8px;
    background: #2563eb; color: #fff; font-size: 16px; font-weight: 700;
  }
  button:disabled { opacity: .5; }
  .err { color: #b91c1c; font-size: 13px; margin: 6px 0; }
  .hint { color: #64748b; font-size: 12px; margin: 8px 0 0; }
</style>
