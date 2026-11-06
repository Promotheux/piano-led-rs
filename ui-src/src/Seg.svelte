<script>
  // A segmented control: one of a few options, the current one highlighted.
  // options: [[value, label], …]; onpick(value) when one is tapped.
  let { options, value, onpick, label, small = false } = $props();
</script>

<div class={['seg', { small }]} role="group" aria-label={label}>
  {#each options as [v, text] (v)}
    <button type="button" class={{ on: v === value }} aria-pressed={v === value} onclick={() => onpick(v)}>{text}</button>
  {/each}
</div>
