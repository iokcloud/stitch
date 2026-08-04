<script lang="ts">
  // 首次启动向导：模型（含连通测试）→ 工作目录 → 开始。
  // 目标：不把新用户丢进完整设置，两步配好即可发第一条消息。
  import { PROVIDER_ORDER, PROVIDER_PRESETS, providerPresetLabel } from "../types";
  import { testConnection, upsertLlmProfile } from "../ipc";
  import { workDir, workDirDialogOpen } from "../stores/app";
  import { ensureSession, ensureSessionLlm } from "../stores/sessions";
  import { nav } from "../nav.svelte";

  let step = $state(1);
  let provider = $state("deepseek");
  let apiKey = $state("");
  let model = $state("");
  let testing = $state(false);
  let testOk = $state(false);
  let testMsg = $state("");
  let saving = $state(false);
  let saveError = $state("");

  const preset = $derived(PROVIDER_PRESETS[provider] ?? PROVIDER_PRESETS.custom);
  const canNext = $derived(apiKey.trim().length > 0 && model.trim().length > 0);

  async function runTest() {
    testing = true;
    testOk = false;
    testMsg = "";
    const ok = await testConnection({
      llm_api_base: preset.api_base,
      llm_model: model.trim(),
      llm_api_key: apiKey.trim(),
    });
    testOk = ok;
    testMsg = ok ? "连接成功，密钥有效" : "连接失败，请检查密钥与模型名";
    testing = false;
  }

  async function finish() {
    saving = true;
    saveError = "";
    try {
      await upsertLlmProfile({
        id: provider,
        provider,
        api_base: preset.api_base,
        api_key: apiKey.trim(),
        model: model.trim(),
      });
      ensureSession();
      ensureSessionLlm();
      nav.showChat("first-run-done");
    } catch (e) {
      saveError = String(e);
      saving = false;
    }
  }

  function skip() {
    nav.showSettings({ firstRun: true });
  }
</script>

<div class="firstrun-overlay" data-testid="firstrun-wizard">
  <div class="firstrun-card">
    <header class="firstrun-head">
      <span class="firstrun-step">{"步骤 "}{step}{" / 2"}</span>
      <button type="button" class="firstrun-skip" onclick={skip}>跳过，去设置</button>
    </header>

    {#if step === 1}
      <section>
        <h1 class="firstrun-title">连接你的模型</h1>
        <p class="firstrun-desc">选择模型服务并填入密钥。密钥只保存在本机。</p>

        <label class="firstrun-label" for="fr-provider">模型服务</label>
        <select id="fr-provider" class="firstrun-select" bind:value={provider}>
          {#each PROVIDER_ORDER as id}
            <option value={id}>{providerPresetLabel(id)}</option>
          {/each}
        </select>

        <label class="firstrun-label" for="fr-key">API 密钥</label>
        <input
          id="fr-key"
          class="firstrun-input"
          data-testid="fr-key"
          type="password"
          placeholder="sk-..."
          bind:value={apiKey}
        />

        <label class="firstrun-label" for="fr-model">模型名称</label>
        <input
          id="fr-model"
          class="firstrun-input"
          data-testid="fr-model"
          list="fr-models"
          placeholder={preset.models[0] ?? "如 deepseek-v4-flash"}
          bind:value={model}
        />
        <datalist id="fr-models">
          {#each preset.models as m}
            <option value={m} />
          {/each}
        </datalist>

        <div class="firstrun-actions">
          <button
            type="button"
            class="btn-ghost"
            disabled={!canNext || testing}
            onclick={runTest}
          >
            {testing ? "测试中…" : "测试连接"}
          </button>
          {#if testMsg}
            <span class:firstrun-ok={testOk} class:firstrun-fail={!testOk}>{testMsg}</span>
          {/if}
          <button
            type="button"
            class="btn-primary"
            disabled={!canNext}
            onclick={() => (step = 2)}
          >
            下一步
          </button>
        </div>
      </section>
    {:else}
      <section>
        <h1 class="firstrun-title">选择工作目录</h1>
        <p class="firstrun-desc">Stitch 只在这个文件夹内读写文件。</p>

        <div class="firstrun-workdir">
          <span class="firstrun-workdir-path">{$workDir || "未选择"}</span>
          <button type="button" class="btn-ghost" onclick={() => workDirDialogOpen.set(true)}>
            选择文件夹
          </button>
        </div>

        <div class="firstrun-actions">
          <button type="button" class="btn-ghost" onclick={() => (step = 1)}>上一步</button>
          <button type="button" class="btn-primary" disabled={saving} onclick={finish}>
            {saving ? "保存中…" : "开始使用"}
          </button>
        </div>
        {#if saveError}
          <p class="firstrun-fail">{saveError}</p>
        {/if}
      </section>
    {/if}
  </div>
</div>

<style>
  .firstrun-overlay {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--color-background) 78%, transparent);
    backdrop-filter: blur(4px);
  }
  .firstrun-card {
    width: min(26rem, calc(100vw - 3rem));
    border: 1px solid var(--color-border-strong);
    border-radius: 14px;
    background: var(--color-surface);
    padding: 1.4rem 1.6rem 1.6rem;
    box-shadow: 0 18px 48px rgb(0 0 0 / 0.18);
  }
  .firstrun-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 1rem;
  }
  .firstrun-step {
    font-size: 11px;
    letter-spacing: 0.06em;
    color: var(--color-muted);
    font-variant-numeric: tabular-nums;
  }
  .firstrun-skip {
    border: 0;
    background: none;
    font-size: 12px;
    color: var(--color-muted);
    cursor: pointer;
  }
  .firstrun-skip:hover {
    color: var(--color-foreground);
  }
  .firstrun-title {
    margin: 0 0 0.3rem;
    font-size: 17px;
    font-weight: 650;
    color: var(--color-foreground);
  }
  .firstrun-desc {
    margin: 0 0 1.1rem;
    font-size: 12.5px;
    color: var(--color-muted);
  }
  .firstrun-label {
    display: block;
    margin: 0.8rem 0 0.3rem;
    font-size: 11px;
    font-weight: 600;
    color: var(--color-muted);
  }
  .firstrun-input,
  .firstrun-select {
    width: 100%;
    border: 1px solid var(--color-border-strong);
    border-radius: 8px;
    background: var(--color-rail);
    color: var(--color-foreground);
    padding: 0.5rem 0.7rem;
    font-size: 13px;
  }
  .firstrun-actions {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin-top: 1.2rem;
  }
  .firstrun-actions .btn-primary {
    margin-left: auto;
  }
  .firstrun-ok {
    font-size: 12px;
    color: var(--color-success);
  }
  .firstrun-fail {
    font-size: 12px;
    color: var(--color-error);
  }
  .firstrun-workdir {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    padding: 0.7rem;
    border: 1px dashed var(--color-border-strong);
    border-radius: 8px;
  }
  .firstrun-workdir-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-family: var(--font-mono);
    color: var(--color-foreground);
  }
</style>
