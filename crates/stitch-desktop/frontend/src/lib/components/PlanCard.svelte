<script lang="ts">
  import type { PlanStep } from "../types";
  import * as ipc from "../ipc";

  interface Props {
    title: string;
    steps: PlanStep[];
    phase: "proposed" | "approved" | "rejected";
    planId?: string;
  }

  let { title, steps: stepsProp, phase, planId }: Props = $props();
  let busy = $state(false);
  let showAll = $state(false);

  /** Guard corrupt localStorage / partial IPC payloads (steps may be missing). */
  const steps = $derived(Array.isArray(stepsProp) ? stepsProp : []);

  const phaseLabel = $derived(
    phase === "approved" ? "已批准" : phase === "rejected" ? "已拒绝" : "待批准",
  );

  const doneCount = $derived(
    steps.filter(
      (s) => s.status === "done" || s.status === "skipped" || s.status === "failed",
    ).length,
  );
  const failedCount = $derived(steps.filter((s) => s.status === "failed").length);
  const progressPct = $derived(steps.length ? Math.round((doneCount / steps.length) * 100) : 0);
  const activeIndex = $derived(steps.findIndex((s) => s.status === "in_progress"));

  const COLLAPSE_AT = 5;
  const visibleSteps = $derived(
    showAll || steps.length <= COLLAPSE_AT ? steps : steps.slice(0, COLLAPSE_AT),
  );

  async function respond(approved: boolean) {
    if (!planId || busy || phase !== "proposed") return;
    busy = true;
    try {
      await ipc.respondPlan(planId, approved);
    } catch (e) {
      console.warn(e);
      busy = false;
    }
  }
</script>

<div class="plan-card" data-testid="plan-card">
  <div class="plan-card-head">
    <div class="min-w-0">
      <span class="plan-card-title truncate">{title}</span>
      {#if phase === "approved" && steps.length > 0}
        <span class="plan-card-meta">
          {doneCount}/{steps.length} 步
          {#if failedCount > 0}
            · {failedCount} 失败
          {:else if activeIndex >= 0}
            · 进行中第 {activeIndex + 1} 步
          {/if}
        </span>
      {/if}
    </div>
    <span class="plan-card-phase">{phaseLabel}</span>
  </div>

  {#if phase === "approved" && steps.length > 0}
    <div class="plan-progress" aria-hidden="true">
      <i style="width: {progressPct}%"></i>
    </div>
  {/if}

  <ol class="plan-steps">
    {#each visibleSteps as step, i}
      <li
        class="plan-step"
        class:is-active={step.status === "in_progress"}
        class:is-done={step.status === "done" || step.status === "skipped"}
        class:is-failed={step.status === "failed"}
      >
        <span class="plan-step-mark" aria-hidden="true">
          {#if step.status === "done"}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M5 12.5l5 5L19 7" />
            </svg>
          {:else if step.status === "in_progress"}
            <span class="spin sm"></span>
          {:else if step.status === "failed"}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M6 6l12 12M18 6L6 18" />
            </svg>
          {:else if step.status === "skipped"}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M6 12h12" />
            </svg>
          {:else}
            <span class="plan-step-dot"></span>
          {/if}
        </span>
        <span class="min-w-0">
          <span class="plan-step-num">{i + 1}.</span>
          {step.description}
        </span>
      </li>
    {/each}
  </ol>

  {#if steps.length > COLLAPSE_AT}
    <button type="button" class="message-expand mx-3 mb-2" onclick={() => (showAll = !showAll)}>
      {showAll ? "收起步骤" : `显示全部 ${steps.length} 步`}
    </button>
  {/if}

  {#if phase === "proposed" && planId}
    <div class="plan-actions">
      <button
        type="button"
        class="plan-btn reject"
        data-testid="plan-reject"
        disabled={busy}
        onclick={() => respond(false)}>拒绝</button
      >
      <button
        type="button"
        class="plan-btn approve"
        data-testid="plan-approve"
        disabled={busy}
        onclick={() => respond(true)}
      >
        {#if busy}
          <span class="spin sm" aria-hidden="true"></span>
          提交中
        {:else}
          批准并执行
        {/if}
      </button>
    </div>
  {/if}
</div>
