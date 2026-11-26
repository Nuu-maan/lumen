<script lang="ts">
  let { html, title }: { html: string; title: string } = $props();
</script>

<iframe class="frame" sandbox="" srcdoc={html} {title} referrerpolicy="no-referrer"></iframe>

<style>
  .frame {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
    background: #fff;
  }
</style>
