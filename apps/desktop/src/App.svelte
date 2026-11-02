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
  let messages = $state([]); // { role: 'user' | 'assistant' | 'tool' | 'error', text, streaming? }
  let draft = $state('');
  let log;

  function scrollToBottom() {
    tick().then(() => {
      if (log) {
        log.scrollTop = log.scrollHeight;
      }
    });
  }

  function appendMessage(role, text) {
    messages.push({ role, text, streaming: false });
    scrollToBottom();
  }

  function appendAssistantDelta(text) {
    const last = messages.at(-1);
    if (last && last.role === 'assistant' && last.streaming) {
      last.text += text;
    } else {
      messages.push({ role: 'assistant', text, streaming: true });
    }
    scrollToBottom();
  }

  function finalizeResponse(text) {
    const last = messages.at(-1);
    if (last && last.role === 'assistant' && last.streaming) {
      last.streaming = false;
      if (text && text.trim().length > 0) {
        last.text = text;
      }
    } else if (text && text.trim().length > 0) {
      messages.push({ role: 'assistant', text, streaming: false });
    }
    scrollToBottom();
  }

  function onAgentEvent(ev) {
    switch (ev.type) {
      case 'state':
        state = ev.state;
        break;
      case 'transcript':
        appendMessage('user', ev.text);
        break;
      case 'assistant_delta':
        appendAssistantDelta(ev.text);
        break;
      case 'response':
        finalizeResponse(ev.text);
        break;
      case 'tool_started':
        appendMessage('tool', `▶ ${ev.name}`);
        break;
      case 'tool_finished':
        appendMessage('tool', `${ev.ok ? '✓' : '✗'} ${ev.name}`);
        break;
      case 'error':
        appendMessage('error', ev.message);
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
    if (!text || state === 'thinking' || state === 'using_tool') return;
    draft = '';
    appendMessage('user', text);
    try {
      await invoke('send_message', { text });
    } catch (e) {
      appendMessage('error', String(e));
    }
  }

  async function toggleListening() {
    if (!voice) return;
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
          Atajo: Ctrl/Cmd + Shift + Espacio
        {:else}
          Modo texto (añade modelos para voz)
        {/if}
      </small>
    </div>
    <button
      onclick={toggleListening}
      disabled={!voice}
      class:on={listening}
      title={voice ? 'Activar/desactivar micrófono' : 'Requiere compilar con soporte de voz'}
    >
      {listening ? '🎙 Activo' : '🎙 Silencio'}
    </button>
  </header>

  <section class="log" bind:this={log}>
    {#each messages as m}
      <div class="msg {m.role}">{m.text}</div>
    {/each}
    {#if messages.length === 0}
      <p class="empty">Escribe un mensaje o activa el micrófono para interactuar.</p>
    {/if}
  </section>

  <form onsubmit={(e) => { e.preventDefault(); send(); }}>
    <input
      bind:value={draft}
      placeholder="Escribe un mensaje…"
      disabled={state === 'thinking' || state === 'using_tool'}
    />
    <button
      type="submit"
      disabled={!draft.trim() || state === 'thinking' || state === 'using_tool'}
    >
      Enviar
    </button>
  </form>
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: #11131a;
    color: #e6e8ef;
  }
  main { display: flex; flex-direction: column; height: 100vh; }
  header { display: flex; align-items: center; gap: 12px; padding: 14px 16px; border-bottom: 1px solid #232634; }
  .status { flex: 1; display: flex; flex-direction: column; }
  small { color: #8b90a3; font-size: 0.8rem; }
  .orb { width: 18px; height: 18px; border-radius: 50%; background: #3a3f55; transition: background 0.2s; }
  .orb.listening { background: #3ddc84; }
  .orb.transcribing, .orb.thinking { background: #ffb020; }
  .orb.using_tool { background: #a78bfa; }
  .orb.speaking { background: #4da3ff; }
  .orb.listening, .orb.speaking, .orb.thinking, .orb.using_tool { animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { transform: scale(1.35); opacity: 0.7; } }
  .log { flex: 1; overflow-y: auto; padding: 16px; display: flex; flex-direction: column; gap: 8px; }
  .empty { margin: auto; color: #6b7087; text-align: center; font-size: 0.95rem; }
  .msg { max-width: 85%; padding: 8px 12px; border-radius: 12px; white-space: pre-wrap; line-height: 1.4; word-break: break-word; }
  .msg.user { align-self: flex-end; background: #2b5cff; color: #ffffff; }
  .msg.assistant { align-self: flex-start; background: #1d2030; color: #e6e8ef; border: 1px solid #272c40; }
  .msg.tool { align-self: flex-start; font-size: 0.82rem; color: #b9a8ff; background: transparent; padding: 0 4px; font-family: monospace; }
  .msg.error { align-self: stretch; background: #3a1d24; color: #ff9aa8; max-width: 100%; border: 1px solid #5a2632; }
  form { display: flex; gap: 8px; padding: 12px 16px; border-top: 1px solid #232634; background: #151822; }
  input { flex: 1; padding: 10px 14px; border-radius: 10px; border: 1px solid #2c3042; background: #1b1e2c; color: inherit; font-size: 0.95rem; }
  input:focus { outline: none; border-color: #2b5cff; }
  button { padding: 8px 16px; border-radius: 10px; border: 0; background: #2b5cff; color: white; cursor: pointer; font-weight: 500; font-size: 0.9rem; }
  button:disabled { opacity: 0.4; cursor: default; }
  header button { background: #2c3042; }
  header button.on { background: #1f7a4d; }
</style>
