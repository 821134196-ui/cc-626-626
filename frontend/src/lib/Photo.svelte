<script>
  import { onMount } from 'svelte';
  import { photoUrl } from '../api.js';

  export let filename;
  export let alt = '现场照片';

  let url = null;
  let failed = false;
  let open = false;

  onMount(() => {
    let revoked = false;
    photoUrl(filename)
      .then((u) => {
        if (!revoked) url = u;
      })
      .catch(() => (failed = true));
    return () => {
      revoked = true;
      if (url) URL.revokeObjectURL(url);
    };
  });
</script>

{#if failed}
  <div class="ph err">照片不可见</div>
{:else if url}
  <button class="photo" type="button" on:click={() => (open = true)}>
    <img src={url} {alt} loading="lazy" />
  </button>
{:else}
  <div class="ph">加载中…</div>
{/if}

{#if open && url}
  <div class="lightbox" on:click={() => (open = false)} role="dialog">
    <img src={url} {alt} />
  </div>
{/if}

<style>
  .photo {
    padding: 0;
    border: 1px solid #d4dae2;
    border-radius: 10px;
    overflow: hidden;
    background: #f3f5f8;
    cursor: zoom-in;
  }
  .photo img {
    display: block;
    width: 100%;
    max-width: 150px;
    height: 96px;
    object-fit: cover;
  }
  .ph {
    width: 150px;
    height: 96px;
    display: grid;
    place-items: center;
    background: #f3f5f8;
    border-radius: 10px;
    color: #8a94a3;
    font-size: 13px;
  }
  .ph.err {
    color: #b3261e;
  }
  .lightbox {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.82);
    display: grid;
    place-items: center;
    z-index: 50;
    padding: 16px;
  }
  .lightbox img {
    max-width: 100%;
    max-height: 100%;
    border-radius: 8px;
  }
</style>
