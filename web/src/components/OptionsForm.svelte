<script lang="ts">
  // A form for the options of a target, made from the defaults the server reports for it:
  // a boolean is a switch, a number a number field, a string a text field, anything else JSON.
  // (Schemas, which would also give choices and ranges, are the open slice 22.)
  let {
    defaults,
    value = $bindable({}),
    invalid = $bindable(false),
  }: {
    defaults: Record<string, unknown>;
    value: Record<string, unknown>;
    invalid: boolean;
  } = $props();

  const keys = $derived(Object.keys(defaults));
  let raw = $state<Record<string, string>>({});
  let bad = $state<Record<string, boolean>>({});

  $effect(() => {
    const next: Record<string, string> = {};
    for (const k of Object.keys(defaults)) {
      const d = defaults[k];
      next[k] = typeof d === 'object' && d !== null ? JSON.stringify(d) : String(d ?? '');
    }
    raw = next;
    bad = {};
    value = { ...defaults };
  });

  $effect(() => {
    invalid = Object.values(bad).some(Boolean);
  });

  function set(key: string, next: unknown) {
    value = { ...value, [key]: next };
  }

  function text(key: string, input: string) {
    raw[key] = input;
    // An optional option left empty is not sent; a number in a text field is a number
    // (`mips` takes 2 as well as "first").
    const t = input.trim();
    set(key, defaults[key] === null && t === '' ? null : /^\d+$/.test(t) ? Number(t) : input);
  }

  function json(key: string, input: string) {
    raw[key] = input;
    try {
      set(key, JSON.parse(input));
      bad[key] = false;
    } catch {
      bad[key] = true;
    }
  }

  function num(key: string, input: string) {
    raw[key] = input;
    const n = Number(input);
    bad[key] = input.trim() === '' || Number.isNaN(n);
    if (!bad[key]) set(key, n);
  }
</script>

{#if keys.length === 0}
  <p class="none">This target has no options.</p>
{:else}
  <div class="form">
    {#each keys as key (key)}
      {@const d = defaults[key]}
      <label class="field" class:bool={typeof d === 'boolean'}>
        <span class="k">{key}</span>
        {#if typeof d === 'boolean'}
          <input type="checkbox" class="switch" checked={Boolean(value[key])} onchange={(e) => set(key, e.currentTarget.checked)} />
        {:else if typeof d === 'number'}
          <input type="text" inputmode="decimal" class:bad={bad[key]} value={raw[key] ?? ''} oninput={(e) => num(key, e.currentTarget.value)} />
        {:else if typeof d === 'object' && d !== null}
          <textarea rows="2" class="mono" class:bad={bad[key]} value={raw[key] ?? ''} oninput={(e) => json(key, e.currentTarget.value)}></textarea>
        {:else}
          <input
            type="text"
            value={raw[key] ?? ''}
            placeholder={d === null ? 'default' : ''}
            oninput={(e) => text(key, e.currentTarget.value)}
          />
        {/if}
      </label>
    {/each}
  </div>
{/if}

<style>
  .form {
    display: grid;
    gap: 12px;
  }
  .field {
    display: grid;
    gap: 5px;
  }
  .field.bool {
    grid-template-columns: 1fr auto;
    align-items: center;
  }
  .k {
    color: var(--text-2);
    font-weight: 600;
    font-size: 12.5px;
  }
  .none {
    color: var(--muted);
    margin: 0;
  }
  .bad {
    border-color: var(--err) !important;
  }
  .switch {
    appearance: none;
    width: 38px;
    height: 22px;
    border-radius: 99px;
    background: var(--line-2);
    position: relative;
    cursor: pointer;
    transition: background 0.15s;
    margin: 0;
  }
  .switch::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 0.15s;
  }
  .switch:checked {
    background: var(--accent);
  }
  .switch:checked::after {
    transform: translateX(16px);
  }
</style>
