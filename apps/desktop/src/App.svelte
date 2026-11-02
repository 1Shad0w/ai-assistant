<script>
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';

  const STATE_LABELS = {
    idle: 'En reposo',
    listening: 'Escuchando…',
    transcribing: 'Transcribiendo…',
    thinking: 'Pensando…',
    using_tool: 'Usando herramienta…',
    speaking: 'Hablando…',
  };

  let state = $state('idle');
  let listening = $state(false);
  let voice = $state(false);
  let messages = $state([]); // { role: 'user' | 'assistant' | 'tool' | 'error', text }
  let draft = $state('');
  let log;

  async function push(role, text) {
    messages.push({ role, text });
    await tick();
    log?.scrollTo({ top: log.scrollHeight });
  }

  function onAgentEvent(ev) {
    switch (ev.type) {
      case 'state':
        state = ev.state;
        break;
      case 'transcript':
        push('user', ev.text);
        break;
      case 'assistant_delta': {
        const last = messages.at(-1);
        if (last?.role === 'assistant' && last.streaming) last.text += ev.text;
        else push('assistant', ev.text).then(() => (messages.at(-1).streaming = true));
        break;
      }
      case 'response': {
        const last = messages.at(-1);
        if (last?.streaming) last.streaming = false;
        else push('assistant', ev.text);
        break;
      }
      case 'tool_started':
        push('tool', `▶ ${ev.name}`);
        break;
      case 'tool_finished':
        push('tool', `${ev.ok ? '✓' : '✗'} ${ev.name}`);
        break;
      case 'error':
        push('error', ev.message);
        break;
    }
  }

  onMount(() => {
    const unlisten = [];
    (async () => {
      voice = (await invoke('capabilities')).voice;
      unlisten.push(await listen('agent-event', (e) => onAgentEvent(e.payload)));
      unlisten.push(await listen('listening-changed', (e) => (listening = e.payload)));
    })();
    return () => unlisten.forEach((u) => u());
  });

  async function send() {
    const text = draft.trim();
    if (!text) return;
    draft = '';
    await push('user', text);
    await invoke('send_message', { text });
  }

  async function toggleListening() {
    listening = await invoke('toggle_listening');
  }
</script>

<main>
  <header>
    <div class="orb {state}" class:active={listening}></div>
    <div class="status">
      <strong>{STATE_LABELS[state] ?? state}</strong>
      <small>
        {#if voice}
          Atajo global: Ctrl/Cmd + Shift + Espacio
        {:else}
          Modo texto (compilado sin voz)
        {/if}
      </small>
    </div>
    <button onclick={toggleListening} disabled={!voice} class:on={listening} title="Activar/desactivar micrófono">
      {listening ? '🎙 Activo' : '🎙 Silencio'}
    </button>
  </header>

  <section class="log" bind:this={log}>
    {#each messages as m}
      <div class="msg {m.role}">{m.text}</div>
    {/each}
    {#if messages.length === 0}
      <p class="empty">Escribe un mensaje o activa el micrófono.</p>
    {/if}
  </section>

  <form onsubmit={(e) => { e.preventDefault(); send(); }}>
    <input bind:value={draft} placeholder="Escribe un mensaje…" disabled={voice} />
    <button type="submit" disabled={voice || !draft.trim()}>Enviar</button>
  </form>
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: system-ui, sans-serif;
    background: #11131a;
    color: #e6e8ef;
  }
  main { display: flex; flex-direction: column; height: 100vh; }
  header { display: flex; align-items: center; gap: 12px; padding: 14px 16px; border-bottom: 1px solid #232634; }
  .status { flex: 1; display: flex; flex-direction: column; }
  small { color: #8b90a3; }
  .orb { width: 18px; height: 18px; border-radius: 50%; background: #3a3f55; transition: background 0.2s; }
  .orb.listening { background: #3ddc84; }
  .orb.transcribing, .orb.thinking { background: #ffb020; }
  .orb.using_tool { background: #a78bfa; }
  .orb.speaking { background: #4da3ff; }
  .orb.listening, .orb.speaking, .orb.thinking { animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { transform: scale(1.35); opacity: 0.7; } }
  .log { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  .empty { margin: auto; color: #6b7087; }
  .msg { max-width: 85%; padding: 8px 12px; border-radius: 12px; white-space: pre-wrap; line-height: 1.4; }
  .msg.user { align-self: flex-end; background: #2b5cff; }
  .msg.assistant { align-self: flex-start; background: #1d2030; }
  .msg.tool { align-self: flex-start; font-size: 0.8rem; color: #b9a8ff; background: transparent; padding: 0 4px; }
  .msg.error { align-self: stretch; background: #3a1d24; color: #ff9aa8; max-width: 100%; }
  form { display: flex; gap: 8px; padding: 12px 16px; border-top: 1px solid #232634; }
  input { flex: 1; padding: 10px 12px; border-radius: 10px; border: 1px solid #2c3042; background: #171a26; color: inherit; }
  button { padding: 8px 14px; border-radius: 10px; border: 0; background: #2b5cff; color: white; cursor: pointer; }
  button:disabled { opacity: 0.4; cursor: default; }
  header button { background: #2c3042; }
  header button.on { background: #1f7a4d; }
</style>

