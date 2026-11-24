import type { Activity, Part, State } from "@/lib/agencia"

const now = () => Math.floor(Date.now() / 1000)

/** Synthetic OpSec-sanitized dataset (used in Demo Mode / ?demo=1 and the static portfolio build). */
export function buildDemoState(): State {
  const t = now()
  return {
    t,
    haiku: { used: 31, cap: 150, asleep: false },
    quota: {
      claude: [
        { name: "Pool A", s: 28, w: 41, sr: "20:15", wr: "Mon 08:00" },
        { name: "Pool B", s: 16, w: 29, sr: "21:00", wr: "Tue 09:00" },
      ],
      codex: [
        { name: "Codex Pool", s: 34, w: 48, sr: t + 7200, wr: t + 259200 },
      ],
      go: [
        { label: "5 h", usd: 1.4, cap: 10, pct: 14 },
        { label: "semana", usd: 11.8, cap: 50, pct: 24 },
      ],
    },
    repos: [
      {
        name: "rust-gateway",
        windows: [
          {
            id: "w:orq-gateway",
            kind: "claude",
            account: "Pool A",
            title: "Orquestador rust-gateway",
            repo: "rust-gateway",
            cwd: "~/workspace/rust-gateway",
            status: "waiting",
            request: "Coordina el pool de conexiones, el caché de rutas y la compresión adaptativa del gateway.",
            now: "¿Aprobamos bajar el pool a 64 conexiones y activar compresión perezosa antes del barrido de carga?",
            steps: [
              { text: "Leyó crates/gateway/src/pool.rs", ic: "read", n: 2, t: t - 120, verb: "Leyó", obj: "pool.rs" },
              { text: "Auditó 0x0040A8C0 en engine.bin", ic: "re", n: 1, t: t - 65, verb: "Auditó", obj: "0x0040A8C0" },
            ],
            errs: 0,
            updated: t - 240,
            model: "claude-opus-4-6",
            usage: { in: 184000, out: 14200, cache: 920000, turns: 18, usd: 1.42 },
            pid: 48210,
            children: [
              {
                id: "cdx:gateway-pool",
                kind: "cdx",
                title: "Métricas del pool de conexiones",
                who: "Codex",
                cuenta: "Codex Pool",
                repo: "rust-gateway",
                request: "Instrumenta el pool de Postgres y elimina los 1.47 GB de presión al GC en pool.rs.",
                status: "working",
                now: "Ejecutando barrido de carga sintético a 10k rps",
                steps: [
                  { text: "Editó crates/gateway/src/pool.rs", ic: "edit", n: 3, t: t - 95, verb: "Editó", obj: "crates/gateway/src/pool.rs" },
                  { text: "Compiló objetivo release", ic: "build", n: 1, t: t - 48, verb: "Compiló", obj: "release" },
                  { text: "Corriendo prueba de latencia p99", ic: "run", n: 1, t: t - 8, verb: "Corre", obj: "latency_bench" },
                ],
                errs: 0,
                updated: t - 8,
                model: "gpt-5.3-codex",
                usage: { in: 92000, out: 8400, cache: 310000, turns: 9 },
                live: true,
                pid: 48902,
                recap: {
                  did: "Eliminó asignaciones por petición en pool.rs y bajó la memoria pico a 0.98 GB.",
                  next: "Verificar cero timeouts en el barrido de carga y cerrar el slice.",
                },
                re: [
                  { t: t - 45, tool: "ghidra", addrs: ["0x00401120", "0x0040A8C0"], why: "Verificando el paso de física y el arranque del motor" },
                ],
              },
              {
                id: "bec:gateway-codec",
                kind: "bec",
                title: "Auditoría de buffers del codec",
                who: "Becario",
                repo: "rust-gateway",
                request: "Revisa fugas en el ring buffer del codec durante reconexiones.",
                status: "working",
                now: "Inspeccionando crates/codec/src/ring_buffer.c líneas 180–290",
                steps: [
                  { text: "Buscó malloc sin free en crates/codec", ic: "search", n: 4, t: t - 32, verb: "Buscó", obj: "crates/codec/*.c" },
                ],
                errs: 0,
                updated: t - 14,
                model: "deepseek-v4.1-flash",
                usage: { in: 44000, out: 3100, cache: 88000, turns: 5, usd: 0.03 },
                live: true,
              },
              {
                id: "sub:gateway-docs",
                kind: "sub",
                title: "Sincronizar evidencias en GOAL.md",
                who: "Explore",
                repo: "rust-gateway",
                request: "Actualiza la sección de evidencias verificadas para M2.",
                status: "done",
                now: "Actualizó GOAL.md con 4 capturas y logs de memoria",
                steps: [],
                errs: 0,
                updated: t - 1100,
                model: "claude-sonnet-4-6",
                usage: { in: 22000, out: 1800, cache: 64000, turns: 3 },
                safe: "Libre hace 18 min",
              },
            ],
          },
          {
            id: "agy:gateway-load",
            kind: "agy",
            account: "Antigravity CLI",
            title: "Antigravity · Perfilado de carga",
            repo: "rust-gateway",
            cwd: "~/workspace/rust-gateway-wt/load-profile",
            status: "working",
            request: "Perfilar consumo de memoria del gateway bajo el tope de 1.0 GB.",
            now: "Validando carga en paralelo de rutas y respuestas diferidas",
            steps: [
              { text: "Editó crates/gateway/src/router.rs", ic: "edit", n: 2, t: t - 40, verb: "Editó", obj: "router.rs" },
              { text: "Verificó pico de memoria 0.96 GB", ic: "run", n: 1, t: t - 11, verb: "Corrió", obj: "mem_check" },
            ],
            errs: 0,
            updated: t - 11,
            model: "gemini-pro",
            usage: { in: 76000, out: 6200, cache: 240000, turns: 8 },
            pid: 50112,
            linked: true,
            children: [],
          },
        ],
      },
      {
        name: "vector-db",
        windows: [
          {
            id: "w:orq-vector",
            kind: "claude",
            account: "Pool B",
            title: "Orquestador vector-db",
            repo: "vector-db",
            cwd: "~/workspace/vector-db",
            status: "working",
            request: "Reconciliar benchmarks (m8_bench -> Engine::think) y la máquina de estado en play-ui.ts.",
            now: "Verificando SPRT (+18 Elo) y 399 pruebas de Rust",
            steps: [
              { text: "Editó apps/web/src/play-state-machine.ts", ic: "edit", n: 2, t: t - 28, verb: "Editó", obj: "play-state-machine.ts" },
              { text: "Ejecutó just bench (4.62M nps)", ic: "run", n: 1, t: t - 6, verb: "Corrió", obj: "just bench" },
            ],
            errs: 0,
            updated: t - 6,
            model: "claude-sonnet-4-6",
            usage: { in: 112000, out: 9800, cache: 540000, turns: 12, usd: 0.68 },
            pid: 51104,
            children: [
              {
                id: "bec:chess-prune",
                kind: "bec",
                title: "Poda de 23 scripts huérfanos",
                who: "Becario",
                repo: "vector-db",
                request: "Verifica qué archivos en scripts/ no tienen invocador en justfile ni package.json.",
                status: "done",
                now: "Confirmó 23 scripts sin invocador (~10,085 líneas)",
                steps: [],
                errs: 0,
                updated: t - 480,
                model: "deepseek-v4.1-flash",
                usage: { in: 29000, out: 2100, cache: 61000, turns: 4, usd: 0.02 },
                live: false,
                safe: "Libre hace 8 min",
              },
            ],
          },
        ],
      },
      {
        name: "log-indexer",
        windows: [
          {
            id: "w:orq-indexer",
            kind: "claude",
            account: "Pool A",
            title: "Orquestador log-indexer",
            repo: "log-indexer",
            cwd: "~/workspace/log-indexer",
            status: "idle",
            request: "Auditoría de indexación incremental, memoria mapeada y recuperación tras caída.",
            now: "Listo tras 4 oleadas (89 hallazgos cerrados)",
            steps: [
              { text: "Verificó mmap y fsync en la escritura", ic: "run", n: 1, t: t - 900, verb: "Verificó", obj: "fsync_check" },
            ],
            errs: 0,
            updated: t - 900,
            model: "claude-opus-4-6",
            usage: { in: 138000, out: 11400, cache: 710000, turns: 14, usd: 1.08 },
            pid: 52301,
            children: [
              {
                id: "cdx:indexer-mmap",
                kind: "cdx",
                title: "Escritor con mmap",
                who: "Codex",
                cuenta: "Codex Pool",
                repo: "log-indexer",
                request: "Corrige el cursor de escritura compartido entre segmentos concurrentes.",
                status: "failed",
                now: "Falló la validación: offset fuera de rango en write_segment",
                steps: [
                  { text: "Editó crates/writer/src/segment.c", ic: "edit", n: 2, t: t - 720, verb: "Editó", obj: "segment.c" },
                ],
                errs: 1,
                updated: t - 700,
                model: "gpt-5.3-codex",
                usage: { in: 58000, out: 4900, cache: 170000, turns: 6 },
                live: false,
              },
            ],
          },
        ],
      },
    ],
    homes: [
      { kind: "cdx", id: "pool", label: "Codex Pool", desc: "Hilo retomable" },
      { kind: "cla", id: "pool-a", label: "Claude", desc: "Worker en repo" },
      { kind: "bec", id: "", label: "DeepSeek", desc: "Cola limpia" },
    ],
    bus: {
      peers: [
        {
          peer_id: "claude:orq-gateway",
          provider: "claude",
          pid: 48210,
          name: "Orquestador rust-gateway",
          repo: "rust-gateway",
          branch: "main",
          cwd: "~/workspace/rust-gateway",
          transport: "claude-uds",
          endpoint: "/tmp/claude-48210.sock",
          title: "Orquestador rust-gateway",
          status: "waiting",
          push_ready: true,
          last_seen: t - 12,
        },
        {
          peer_id: "agy:gateway-load",
          provider: "agy",
          pid: 50112,
          name: "Antigravity · Perfilado de carga",
          repo: "rust-gateway",
          branch: "load-profile",
          cwd: "~/workspace/rust-gateway-wt/load-profile",
          transport: "agy-agentapi",
          endpoint: "http://127.0.0.1:41902",
          title: "Perfilado de carga",
          status: "working",
          push_ready: true,
          last_seen: t - 8,
        },
        {
          peer_id: "codex:gateway-pool",
          provider: "codex",
          pid: 48902,
          name: "Codex · Pool de conexiones",
          repo: "rust-gateway",
          branch: "main",
          cwd: "~/workspace/rust-gateway",
          transport: "bus-inbox",
          title: "Métricas del pool de conexiones",
          status: "working",
          push_ready: false,
          last_seen: t - 5,
        },
        {
          peer_id: "claude:orq-vector",
          provider: "claude",
          pid: 51104,
          name: "Orquestador vector-db",
          repo: "vector-db",
          branch: "main",
          cwd: "~/workspace/vector-db",
          transport: "claude-uds",
          endpoint: "/tmp/claude-51104.sock",
          title: "Orquestador vector-db",
          status: "working",
          push_ready: true,
          last_seen: t - 6,
        },
      ],
      threads: [
        {
          thread_id: "thr-gateway-pool",
          topic: "Pool de 64 conexiones y compresión diferida",
          created_by: "agy:gateway-load",
          created_at: t - 180,
          hops_left: 4,
          max_hops: 6,
          status: "open",
        },
        {
          thread_id: "thr-chess-bench",
          topic: "Reconciliación m8_bench y RequestTracker",
          created_by: "claude:orq-vector",
          created_at: t - 420,
          hops_left: 0,
          max_hops: 4,
          status: "done",
        },
      ],
      messages: [
        {
          msg_id: 1,
          thread_id: "thr-gateway-pool",
          seq: 1,
          from_peer: "agy:gateway-load",
          to_peer: "claude:orq-gateway",
          body: "Pico de memoria bajó a 0.96 GB con compresión diferida. ¿Fijamos el pool en 64 conexiones?",
          expect_reply: true,
          delivered_via: "claude-uds",
          delivered_ok: true,
          created_at: t - 160,
          read_at: t - 140,
        },
        {
          msg_id: 2,
          thread_id: "thr-gateway-pool",
          seq: 2,
          from_peer: "claude:orq-gateway",
          to_peer: "codex:gateway-pool",
          body: "Valida cero regresiones de latencia p99 en el barrido de 10k rps con el límite de 64 conexiones.",
          expect_reply: true,
          delivered_via: "bus-inbox",
          delivered_ok: true,
          created_at: t - 90,
          read_at: null,
        },
        {
          msg_id: 3,
          thread_id: "thr-chess-bench",
          seq: 1,
          from_peer: "claude:orq-vector",
          to_peer: "claude:orq-gateway",
          body: "399 pruebas Rust y SPRT (+18 Elo) en verde tras invalidar las 4 generaciones.",
          expect_reply: false,
          delivered_via: "claude-uds",
          delivered_ok: true,
          created_at: t - 360,
          read_at: t - 350,
        },
      ],
    },
    avance: {
      "rust-gateway": {
        cur: "M2",
        agree: 9,
        votes: 10,
        updated: t - 90,
        busy: false,
        pending: false,
        err: "",
        runs: 22,
        orq: "orq-gateway",
        goal: [
          { id: "M1", state: "VERIFIED", done_when: "13,770 líneas de C del codec compiladas a WASM sin emulador", sees: "Corre en Chrome y Safari", ev: "tests/boot.log" },
          { id: "M2", state: "UNVERIFIED", done_when: "Arranque paralelo + perfilado de carga debajo de 1.0 GB sin picos de GC", sees: "0.96 GB medidos en el barrido", ev: "" },
          { id: "M3", state: "UNVERIFIED", done_when: "Latencia p99 estable a 10k rps en el arranque y en producción", sees: "En validación", ev: "" },
        ],
        tasks: [
          {
            key: "gateway-1",
            who: "Codex",
            kind: "codex",
            title: "Eliminar 1.47 GB de presión al GC en pool.rs",
            line: "L1",
            ms: "M2",
            done_when: "Cero asignaciones de buffer por petición en el connection pool",
            done: "",
            last: { who: "Codex", state: "pass", line: "Buffers reutilizables verificados durante 60 s a 10k rps", ev: "", t: t - 45 },
            audit: { v: "sirve", claim: "coincide", why: "Abrió el diff en crates/gateway/src/pool.rs y el log de heap: presión de GC cayó 94 %.", t: t - 30, shot: "" },
            vote: true,
          },
          {
            key: "gateway-2",
            who: "Becario",
            kind: "bec",
            title: "Corregir bug de 375 ms en el arranque del gateway",
            line: "L2",
            ms: "M2",
            done_when: "Arranque sincronizado sin disparar CPU al 100 %",
            done: "",
            last: { who: "Becario", state: "done", line: "Agregué un límite de dt en el bucle de arranque", ev: "", t: t - 520 },
            audit: { v: "no", claim: "exagera", why: "El límite acota dt pero el bucle sigue llamando flush() sin esperar al temporizador.", t: t - 460, shot: "" },
            vote: null,
          },
        ],
      },
      "vector-db": {
        cur: "M3",
        agree: 8,
        votes: 8,
        updated: t - 140,
        busy: false,
        pending: false,
        err: "",
        runs: 14,
        orq: "orq-vector",
        goal: [
          { id: "M1", state: "VERIFIED", done_when: "Motor PVS en Rust (~3039 Elo CCRL Blitz) compilado a WASM sin red", sees: "1.0–1.5M nodos/s en pestaña", ev: "tests/sprt.log" },
          { id: "M2", state: "VERIFIED", done_when: "just bench mide Engine::think en vez de la búsqueda vieja", sees: "4.62M nps a prof. 9 en 42 ms", ev: "benches/baseline.json" },
          { id: "M3", state: "UNVERIFIED", done_when: "RequestTracker atómico y 0 errores tragados en workers", sees: "Pruebas verdes", ev: "" },
        ],
        tasks: [
          {
            key: "chess-1",
            who: "Orquestador",
            kind: "session",
            title: "createRequestTracker() en play-state-machine.ts",
            line: "L1",
            ms: "M3",
            done_when: "resignGame invalida las 4 generaciones atómicamente",
            done: "",
            last: { who: "Orquestador", state: "pass", line: "399 pruebas Rust + 15 suites web verdes", ev: "", t: t - 190 },
            audit: { v: "sirve", claim: "coincide", why: "Verificó invalidateAll() en play-state-machine.ts y su prueba de carrera.", t: t - 150, shot: "" },
            vote: true,
          },
        ],
      },
    },
  }
}

export function buildDemoParts(id: string): { parts: Part[]; reply: string; request: string } {
  const t = now()
  return {
    request: "Valida los cambios recientes con evidencia reproducible antes de cerrar el slice.",
    reply:
      "RESUMEN: Validación completada en el entorno de demostración.\n\nHECHO:\n- Se revisaron los archivos clave del frente `" +
      id +
      "`.\n- Se corrieron las pruebas sin regresiones ni fugas.\n\nSIGUIENTE:\n- Esperando luz verde del OWNER para integrar a main.",
    parts: [
      { t: t - 180, role: "user", kind: "text", text: "Valida los cambios recientes y deja evidencia limpia en el log." },
      { t: t - 140, role: "assistant", kind: "tool", text: "", name: "Read", arg: "crates/gateway/src/pool.rs", ok: true, out: "// 240 lines read cleanly" },
      { t: t - 95, role: "assistant", kind: "tool", text: "", name: "Edit", arg: "crates/gateway/src/pool.rs", ok: true, out: "Lazy compression enabled; peak heap reduced to 0.96 GB" },
      { t: t - 42, role: "assistant", kind: "tool", text: "", name: "Bash", arg: "git diff --stat", ok: true, out: " crates/gateway/src/pool.rs | 28 ++++++++++++++--------------\n 1 file changed, 14 insertions(+), 14 deletions(-)" },
      { t: t - 15, role: "assistant", kind: "text", text: "Listo el ajuste de compresión perezosa. Quedo atento a tu señal por relay directo para lanzar el barrido completo." },
    ],
  }
}

export function buildDemoActivity(): Activity {
  const t = now()
  return {
    by_model: [
      { model: "claude-opus-4-6", in: 420000, out: 34000, cache: 1850000, agents: 2 },
      { model: "gpt-5.3-codex", in: 290000, out: 26000, cache: 840000, agents: 2 },
      { model: "gemini-pro", in: 148000, out: 12000, cache: 410000, agents: 1 },
      { model: "deepseek-v4.1-flash", in: 180000, out: 14000, cache: 310000, agents: 4 },
    ],
    by_account: [
      { account: "Pool A (Claude)", in: 322000, out: 25600, agents: 2 },
      { account: "Codex Pool", in: 150000, out: 13300, agents: 2 },
      { account: "Antigravity CLI", in: 76000, out: 6200, agents: 1 },
      { account: "Fast Pool (OpenCode)", in: 73000, out: 5200, agents: 2 },
    ],
    agents: [
      { id: "w:orq-gateway", title: "Orquestador rust-gateway", who: "Claude", model: "claude-opus-4-6", account: "Pool A", in: 184000, out: 14200, updated: t - 240 },
      { id: "cdx:gateway-pool", title: "Métricas del pool de conexiones", who: "Codex", model: "gpt-5.3-codex", account: "Codex Pool", in: 92000, out: 8400, updated: t - 8 },
      { id: "agy:gateway-load", title: "Antigravity · Perfilado de carga", who: "Antigravity", model: "gemini-pro", account: "Antigravity CLI", in: 76000, out: 6200, updated: t - 11 },
      { id: "w:orq-vector", title: "Orquestador vector-db", who: "Claude", model: "claude-sonnet-4-6", account: "Pool B", in: 112000, out: 9800, updated: t - 6 },
      { id: "bec:gateway-codec", title: "Auditoría de buffers del codec", who: "Becario", model: "deepseek-v4.1-flash", account: "Fast Pool", in: 44000, out: 3100, updated: t - 14 },
    ],
  }
}

/** Synthetic Agencia Real (Slices) dataset for static demo mode. */
export function buildDemoRun() {
  const t = now()
  return {
    ok: true,
    state: "on" as const,
    repo: "rust-gateway",
    goal: "Bajar el arranque a <3 s y la memoria del gateway debajo de 1.0 GB",
    started: t - 4200,
    until: t + 10200,
    pct: 30,
    haiku: { awake: true, used: 31, cap: 150 },
    accounts: [
      { id: "claude:a", name: "Pool A", spent: 11, over: false },
      { id: "codex:pool", name: "Codex Pool", spent: 14, over: false },
      { id: "go", name: "Fast Pool", spent: 6, over: false },
    ],
    slices: [
      {
        name: "pool-tuning",
        path: "~/workspace/rust-gateway-wt/pool-tuning",
        ahead: 3,
        dirty: 2,
        last: "compresión perezosa + pool de 64 conexiones",
        age: 18,
        claude: 1,
        codex: 1,
        bec: 1,
        running: 2,
        queued: 0,
        done: 4,
        failed: 0,
        steps: 42,
        tokens: 248000,
        usd: 0.84,
        added: 186,
        removed: 94,
        files_n: 3,
        files: [
          { p: "crates/gateway/src/pool.rs", name: "pool.rs", add: 112, rem: 48, isNew: false, size: 8420, t: t - 40, view: false },
          { p: "crates/gateway/src/router.rs", name: "router.rs", add: 74, rem: 46, isNew: false, size: 5190, t: t - 110, view: false },
        ],
        commits: [
          { hash: "a91c4e2", subject: "gateway: lazy response compression on first hit", t: t - 600 },
          { hash: "7b20f18", subject: "pool: shrink postgres pool to 64 connections", t: t - 1500 },
        ],
        jobs: [
          {
            id: "131402a1",
            who: "Codex",
            brief: "Eliminar presión al GC en pool.rs",
            status: "r" as const,
            model: "gpt-5.3-codex",
            steps: 14,
            secs: 420,
            line: "Corriendo prueba de latencia p99 a 10k rps",
            reply: "",
            refs: [],
            tokens: 92000,
            usd: 0.34,
            t: t - 8,
          },
        ],
        story: [
          {
            t: t - 120,
            did: "Partió el arranque en paralelo y redujo la reserva de memoria a 1024 MB.",
            now: "Midiendo memoria pico del gateway (0.96 GB).",
            next: "Integrar rama slice/pool-tuning si pasa el barrido completo.",
          },
        ],
        story_err: "",
      },
      {
        name: "codec-buffer",
        path: "~/workspace/rust-gateway-wt/codec-buffer",
        ahead: 1,
        dirty: 1,
        last: "ring buffer bounds check",
        age: 64,
        claude: 0,
        codex: 0,
        bec: 2,
        running: 1,
        queued: 0,
        done: 2,
        failed: 0,
        steps: 19,
        tokens: 68000,
        usd: 0.05,
        added: 44,
        removed: 19,
        files_n: 1,
        files: [
          { p: "crates/codec/src/ring_buffer.c", name: "ring_buffer.c", add: 44, rem: 19, isNew: false, size: 6120, t: t - 64, view: false },
        ],
        commits: [
          { hash: "3f8d210", subject: "codec: guard ring buffer underrun on reconnect", t: t - 1200 },
        ],
        jobs: [],
        story: [
          {
            t: t - 180,
            did: "Corrigió underrun del ring buffer durante reconexiones.",
            now: "Auditando asignaciones en crates/codec/src/ring_buffer.c.",
            next: "Validar 0 glitches de audio en captura headless.",
          },
        ],
        story_err: "",
      },
    ],
  }
}

/** Synthetic Lupa RE (Ghidra decompile) dataset for static demo mode. */
export function buildDemoRe() {
  const targets = [
    { id: "engine-init", img: "engine.bin", proc: "Engine_Init", addr: "0x00401120", name: "Engine_Init", area: "Boot / Init" },
    { id: "engine-step", img: "engine.bin", proc: "Physics_Step", addr: "0x0040A8C0", name: "Physics_Step", area: "Physics" },
  ]
  const details: Record<string, { target: (typeof targets)[number]; code: string; ms: number; created: boolean; plain: string }> = {
    "engine-init": {
      target: targets[0],
      ms: 42,
      created: false,
      code: `// 0x00401120 · engine.bin (Engine_Init)
int Engine_Init(EngineConfig *cfg) {
  if (cfg == 0 || cfg->abi != ENGINE_ABI_VERSION) return -1;
  g_clock = ClockOpenMonotonic();
  if (g_clock == 0) return -2;
  g_arena = ArenaCreate(cfg->heap_bytes);
  if (g_arena == 0) return -3;
  g_workers = WorkerPoolStart(cfg->threads);
  // ABI verified -> engine ready for the first Physics_Step
  return g_workers ? 0 : -4;
}`,
      plain: "Inicializa el motor: valida la versión del ABI, abre el reloj monotónico, reserva el arena de memoria y levanta el pool de hilos de trabajo.",
    },
    "engine-step": {
      target: targets[1],
      ms: 38,
      created: false,
      code: `// 0x0040A8C0 · engine.bin (Physics_Step)
void Physics_Step(World *w, float dt) {
  const float h = clamp(dt, 0.0f, w->max_dt);
  for (u32 i = 0; i < w->body_count; i++) {
    Body *b = &w->bodies[i];
    b->vel = vadd(b->vel, vscale(w->gravity, h));
    b->vel = vclamp_len(b->vel, w->max_speed);
    b->pos = vadd(b->pos, vscale(b->vel, h));
  }
  w->accumulator = 0.0f;
}`,
      plain: "Avanza la simulación con paso fijo: integra la gravedad, acota la velocidad máxima de cada cuerpo y actualiza las posiciones del mundo.",
    },
  }
  return { targets, details }
}
