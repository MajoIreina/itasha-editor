// Stable renderer-side lifecycle contract; desktop code never edits project state.
(() => {
  window.itashaLifecycle = {
    async prepareClose() {
      try {
        if (window.studio && studio.stage !== 'home') await studio.save(true);
        return { ok: true };
      } catch (error) { return { ok: false, error: error.message }; }
    }
  };
})();
