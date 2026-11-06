<script>
  // Shows an app once the Pi has sent its state (the first live update).
  import { apps } from './apps.svelte.js';

  let { app, view: View } = $props();
</script>

{#if apps[app]}
  <View />
{/if}
