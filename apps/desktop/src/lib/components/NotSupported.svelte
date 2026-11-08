<script lang="ts">
  /**
   * The web edition in a browser without WebUSB and WebHID (FR QFTCX1, AC .3): names the
   * browsers that work, links the desktop app and points at the demo meter, which still
   * runs here.
   */
  import { RELEASES_URL } from "../web/features";
</script>

<section class="not-supported" aria-labelledby="not-supported-title">
  <h2 id="not-supported-title">This browser is not supported</h2>
  <p>
    Connecting a meter needs WebUSB or WebHID: open this page in Chrome or Edge, or get the
    <a href={RELEASES_URL} target="_blank" rel="noopener noreferrer">desktop app</a>.
  </p>
  <p>The demo meter still works here: pick it in the device list to try the app.</p>
</section>

<style>
  .not-supported {
    margin: 0.75rem 1rem;
    padding: 0.75rem 1rem;
    border: 1px solid var(--border, currentColor);
    border-radius: 6px;
  }
  h2 {
    margin: 0 0 0.4em;
    font-size: 1.05em;
  }
  p {
    margin: 0.25em 0;
  }
</style>
