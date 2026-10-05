<script>
  import { onMount } from 'svelte'
  import { fetchPhotoUrl } from './api.js'

  export let reportId
  let src = ''
  let failed = false

  onMount(async () => {
    try { src = await fetchPhotoUrl(reportId) } catch { failed = true }
  })
</script>

{#if src}
  <img {src} alt="现场照片" />
{:else if failed}
  <div class="ph">照片加载失败（无权限或已删除）</div>
{:else}
  <div class="ph">照片加载中…</div>
{/if}

<style>
  img { width: 100%; border-radius: 8px; display: block; }
  .ph { padding: 24px; text-align: center; color: #9aa5b1; background: #f5f7fa; border-radius: 8px; font-size: 13px; }
</style>
