import { useEffect, useRef, useState } from "react"
import { ArrowDown, ArrowUp, ChevronRight, Eraser, Pause, Play, Power, SquareTerminal, X } from "lucide-react"
import { toast } from "sonner"
import { Age, StatusTag } from "@/components/bits"
import { Brand } from "@/components/brand"
import { dur, InformePane } from "@/components/inspector/InformePane"
import { isWork, kindOf, post, usd, type Card, type Part, type Win } from "@/lib/agencia"
import { buildDemoParts } from "@/lib/demo"
import { base, isWin, labelOf, lineOf, nameOf, toneOf, wordOf, type Item } from "@/lib/tone"
import { cn } from "@/lib/utils"

export function canControl(it: Item): { pause?: string; clean?: string; close?: string } {
  if (isWin(it)) {
    if (it.kind !== "claude") {
      return {
        pause: "Vive en tu terminal: páusala ahí",
        clean: "Vive en tu terminal: límpiala ahí",
        close: "Vive en tu terminal: ciérrala ahí",
      }
    }
    return {
      clean:
        it.status === "idle" && !it.paused
          ? undefined
          : it.paused
            ? "Está en pausa: reanúdala primero"
            : "Solo se limpia una ventana libre",
    }
  }
  if (it.kind === "sub") {
    const run = it.status === "working"
    return {
      pause: "Corre dentro de su ventana: no tiene proceso propio que pausar",
      clean: run ? "Sigue trabajando: ciérralo primero" : undefined,
      close: run ? undefined : "Ya terminó: límpialo",
    }
  }
  const live = !!it.live
  if (it.kind === "bec") {
    return {
      pause: "Vive en OpenCode: se puede cerrar, no pausar",
      clean: live ? "Sigue corriendo: ciérralo primero" : undefined,
      close: live ? undefined : "Ya terminó",
    }
  }
  return {
    pause: live ? undefined : "Ya terminó",
    clean: live ? "Sigue corriendo: ciérralo primero" : undefined,
    close: live ? undefined : "Ya terminó",
  }
}

type Full = { parts?: Part[]; feed?: { t: number; ic: string; text: string }[]; reply?: string; request?: string }
const WRITES = new Set(["Edit", "Write", "NotebookEdit", "apply_patch"])
const clock = (t: number) => new Date(t * 1000).toLocaleTimeString("es", { hour12: false })

const inlineMd = (t: string) =>
  t.split(/(\*\*[^*\n]+\*\*|`[^`\n]+`)/g).map((x, i) =>
    x.startsWith("**") && x.endsWith("**") && x.length > 4 ? (
      <strong key={i} className="font-semibold text-ink-1">
        {x.slice(2, -2)}
      </strong>
    ) : x.startsWith("`") && x.endsWith("`") && x.length > 2 ? (
      <code key={i} className="rounded border border-line bg-inset px-1 py-0.5 font-mono text-[12px] text-ink-1">
        {x.slice(1, -1)}
      </code>
    ) : (
      x
    ),
  )

export function AgentInspector({
  it,
  demoMode,
  onClose,
}: {
  it: Item
  demoMode?: boolean
  onClose: () => void
}) {
  const [full, setFull] = useState<Full | null>(null)
  const [arm, setArm] = useState<"" | "clean" | "close">("")
  const [msg, setMsg] = useState("")
  const [busy, setBusy] = useState("")
  const [openRow, setOpenRow] = useState<number | null>(null)
  const [tab, setTab] = useState<"informe" | "terminal">(isWork(it) ? "terminal" : "informe")
  const [following, setFollowing] = useState(true)
  const logRef = useRef<HTMLDivElement>(null)

  const id = it.id
  const c = canControl(it)
  const win = isWin(it)
  const tone = toneOf(it)

  useEffect(() => {
    setFull(null)
    setArm("")
    setOpenRow(null)
    setFollowing(true)
    setTab(isWork(it) ? "terminal" : "informe")
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id])

  useEffect(() => {
    if (demoMode) {
      setFull(buildDemoParts(id))
      return
    }
    let dead = false
    fetch(`/api/agent?id=${encodeURIComponent(id)}`)
      .then((r) => (r.ok ? r.json() : null))
      .then((j) => !dead && j && setFull(j))
      .catch(() => {})
    return () => {
      dead = true
    }
  }, [id, it.updated, it.status, demoMode])

  useEffect(() => {
    const el = logRef.current
    if (el && following && tab === "terminal") {
      el.scrollTop = el.scrollHeight
    }
  }, [full, it.steps, tab, following])

  useEffect(() => {
    if (!arm) return
    const h = setTimeout(() => setArm(""), 3000)
    return () => clearTimeout(h)
  }, [arm])

  async function act(action: "pause" | "resume" | "clean" | "close") {
    if (demoMode) {
      toast.info(`Acción "${action}" simulada en Modo Demo`)
      return
    }
    if ((action === "clean" || action === "close") && arm !== action) {
      setArm(action)
      return
    }
    setArm("")
    setBusy(action)
    const r = await post("/api/control", { id, action })
    setBusy("")
    if (r.ok) toast.success(r.info || "Hecho")
    else toast.error("No se pudo ejecutar", { description: r.info })
  }

  const sendable = win && (it.kind === "claude" || (it.kind === "agy" && !!(it as Win).linked)) && !it.paused
  async function send() {
    const text = msg.trim()
    if (!text || !sendable) return
    if (demoMode) {
      setMsg("")
      toast.success(`Mensaje simulado a ${it.title}`)
      return
    }
    setBusy("send")
    const r = await post("/api/send", { to: it.kind === "agy" ? id : id.slice(2), text })
    setBusy("")
    if (r.ok) {
      setMsg("")
      toast.success(`Enviado a ${it.title}`)
    } else {
      toast.error("No llegó el mensaje", { description: r.info })
    }
  }

  const parts = full?.parts ?? []
  const steps = full?.feed ?? it.steps
  const repo = (it as Card).repo ?? ""
  const cwd = win ? it.cwd : repo && !repo.includes("/") ? `~/${repo}` : repo
  const metaParts = [
    base(cwd ?? ""),
    win ? (it as Win).account : (it as Card).cuenta ?? "",
    it.pid ? `pid ${it.pid}` : "",
    it.usage?.usd ? usd(it.usage.usd) : "",
  ].filter(Boolean)

  const last = parts.length ? Math.max(...parts.map((p) => p.t)) : 0
  const first = parts.length ? Math.min(...parts.map((p) => p.t)) : 0
  const pauseBlocked = c.pause && !it.paused
  const toolCount = parts.length ? parts.filter((p) => p.kind === "tool").length : steps.length

  const jumpToStep = (i: number) => {
    setTab("terminal")
    setOpenRow(i)
    setFollowing(false)
    setTimeout(() => {
      logRef.current?.querySelector(`[data-step="${i}"]`)?.scrollIntoView({ block: "center", behavior: "smooth" })
    }, 60)
  }

  return (
    <div className="flex h-full flex-col bg-surface">
      {/* Inspector Header */}
      <header data-tone={tone} className="ag-inspector-head flex flex-col gap-3 border-b border-line px-4 py-3.5">
        <div className="flex items-start gap-2.5">
          <div className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-lg border border-line bg-surface">
            <Brand kind={kindOf(it)} model={it.model} className="size-4.5" />
          </div>
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-[15px] font-semibold tracking-tight text-ink-1" title={it.title}>
              {labelOf(it)}
            </h2>
            <div className="mt-1 flex flex-wrap items-center gap-x-2.5 gap-y-0.5 font-mono text-[12px] tabular-nums text-ink-3">
              {metaParts.map((m, i) => (
                <span key={i}>{m}</span>
              ))}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Cerrar inspector"
            className="pressable grid size-9 shrink-0 place-items-center rounded-lg border border-line-strong bg-surface text-ink-2 hover:bg-hover hover:text-ink-1"
          >
            <X className="size-4" />
          </button>
        </div>

        {/* Status Bar */}
        <div className="flex items-center gap-2.5 rounded-md border border-line bg-surface px-2.5 py-2">
          <StatusTag label={it.paused ? "En pausa" : wordOf(it)} tone={tone} paused={it.paused} />
          <p className="min-w-0 flex-1 truncate text-[13px] text-ink-2">
            {it.paused ? (
              "Proceso detenido. Reanudar lo continúa donde iba."
            ) : tone === "work" || tone === "need" || tone === "bad" || it.status === "stalled" ? (
              lineOf(it)
            ) : (
              <>
                Terminó hace <Age t={it.updated} />
                {first > 0 && last > first ? ` (${dur(last - first)})` : ""}
              </>
            )}
          </p>
        </div>

        {/* Controls */}
        <div className="flex flex-wrap items-center gap-1.5">
          <ActionBtn
            icon={it.paused ? <Play className="size-3.5" /> : <Pause className="size-3.5" />}
            label={it.paused ? "Reanudar" : "Pausar"}
            why={pauseBlocked ? c.pause : undefined}
            busy={busy === "pause" || busy === "resume"}
            onClick={() => act(it.paused ? "resume" : "pause")}
          />
          <ActionBtn
            icon={<Eraser className="size-3.5" />}
            label={arm === "clean" ? "Confirmar" : "Limpiar"}
            why={c.clean}
            busy={busy === "clean"}
            armed={arm === "clean"}
            onClick={() => act("clean")}
            tip={win ? "Empieza de cero (/clear)" : "Saca del tablero y archiva hilo"}
          />
          <ActionBtn
            icon={<Power className="size-3.5" />}
            label={arm === "close" ? "Confirmar" : "Cerrar"}
            why={c.close}
            busy={busy === "close"}
            armed={arm === "close"}
            danger
            onClick={() => act("close")}
            tip={it.kind === "sub" ? "Detiene el subagente" : "Termina el proceso"}
          />
          {!win && it.kind === "cdx" && (
            <ActionBtn
              icon={<SquareTerminal className="size-3.5" />}
              label="Terminal"
              busy={busy === "open"}
              why={isWork(it) || it.status === "queued" ? "Disponible cuando termine" : undefined}
              tip="Abrir hilo en Terminal con codex resume"
              onClick={async () => {
                if (demoMode) {
                  toast.info("Abrir en Terminal simulado en Demo")
                  return
                }
                setBusy("open")
                const r = await post("/api/codex/open", { id })
                setBusy("")
                if (r.ok) toast.success(r.info || "Abierto en Terminal")
                else toast.error("No se pudo abrir", { description: r.info })
              }}
            />
          )}
        </div>

        {/* Dedicated Full-Width Segmented Tab Strip */}
        <div role="tablist" aria-label="Vista del agente" className="grid w-full grid-cols-2 rounded-md border border-line bg-inset p-0.5">
          <button
            type="button"
            role="tab"
            aria-selected={tab === "informe"}
            onClick={() => setTab("informe")}
            className={cn(
              "pressable flex h-8 items-center justify-center rounded-[5px] px-2.5 text-[12.5px] font-medium",
              tab === "informe" ? "bg-surface text-ink-1 shadow-[var(--shadow-sm)]" : "text-ink-3 hover:text-ink-1",
            )}
          >
            Informe
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "terminal"}
            onClick={() => setTab("terminal")}
            className={cn(
              "pressable flex h-8 items-center justify-center gap-1.5 rounded-[5px] px-2.5 text-[12.5px] font-medium",
              tab === "terminal" ? "bg-surface text-ink-1 shadow-[var(--shadow-sm)]" : "text-ink-3 hover:text-ink-1",
            )}
          >
            Terminal
            {toolCount > 0 && <span className="font-mono text-[12px] tabular-nums text-ink-3">{toolCount}</span>}
          </button>
        </div>
      </header>

      {/* Body */}
      {tab === "informe" ? (
        <div className="min-h-0 flex-1 overflow-y-auto" role="tabpanel">
          {full ? (
            <InformePane
              parts={parts}
              reply={full.reply ?? ""}
              request={full.request ?? it.request ?? ""}
              recap={"recap" in it ? it.recap : null}
              shots={it.shots}
              name={nameOf(it)}
              cwd={cwd}
              updated={it.updated}
              working={isWork(it)}
              you={win}
              cap={parts.length >= 80}
              jump={jumpToStep}
            />
          ) : (
            <div className="p-5 text-[13px] text-ink-3">Leyendo historial del agente…</div>
          )}
        </div>
      ) : (
        <div className="relative min-h-0 flex-1 bg-inset">
          <div
            ref={logRef}
            role="log"
            aria-live="polite"
            onScroll={(e) => {
              const el = e.currentTarget
              setFollowing(el.scrollHeight - el.scrollTop - el.clientHeight < 56)
            }}
            className="h-full overflow-y-auto px-3 py-3 font-mono text-[12px]"
          >
            <div className="flex flex-col gap-1">
              {parts.length > 0
                ? parts.map((p, i) =>
                    p.kind === "tool" ? (
                      <div
                        key={i}
                        data-step={i}
                        className={cn(
                          "rounded-md border px-2.5 py-1.5",
                          p.ok === false
                            ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)]"
                            : WRITES.has(p.name ?? "")
                              ? "border-[var(--sig-work-border)] bg-[var(--sig-work-soft)]"
                              : "border-line bg-surface",
                        )}
                      >
                        <button
                          type="button"
                          onClick={() => setOpenRow(openRow === i ? null : i)}
                          className="flex w-full items-start gap-2 text-left"
                        >
                          <ChevronRight
                            className={cn("mt-0.5 size-3.5 shrink-0 text-ink-3 transition-transform duration-150", openRow === i && "rotate-90")}
                          />
                          <span className="shrink-0 text-[12px] tabular-nums text-ink-3">{clock(p.t)}</span>
                          <span className="min-w-0 flex-1 break-words [overflow-wrap:anywhere]">
                            <span className={cn("font-semibold", p.ok === false ? "text-bad" : WRITES.has(p.name ?? "") ? "text-work" : "text-ink-1")}>
                              {p.name}
                            </span>{" "}
                            <span className="text-ink-2">{p.arg}</span>
                          </span>
                        </button>
                        {p.out && (
                          <pre
                            className={cn(
                              "mt-1.5 ml-5 border-l border-line pl-2.5 text-[12px] leading-relaxed text-ink-2 whitespace-pre-wrap break-words [overflow-wrap:anywhere]",
                              openRow !== i && "line-clamp-2",
                            )}
                          >
                            {p.out}
                          </pre>
                        )}
                      </div>
                    ) : (
                      <div
                        key={i}
                        className={cn(
                          "rounded-md px-2.5 py-2 font-sans text-[13px] leading-relaxed",
                          p.role === "user"
                            ? "border border-line-strong bg-elevated text-ink-1"
                            : "border-l-2 border-line-strong bg-surface/60 text-ink-2",
                        )}
                      >
                        <div className="mb-0.5 flex items-center justify-between font-mono text-[12px] text-ink-3">
                          <span>{p.role === "user" ? "OWNER" : nameOf(it)}</span>
                          <span className="tabular-nums">{clock(p.t)}</span>
                        </div>
                        <div className="whitespace-pre-wrap">{inlineMd(p.text)}</div>
                      </div>
                    ),
                  )
                : steps.map((s, i) => (
                    <div key={i} className="flex items-baseline gap-2.5 rounded border border-line bg-surface px-2.5 py-1.5">
                      <span className="shrink-0 text-[12px] tabular-nums text-ink-3">{clock(s.t)}</span>
                      <span className="text-ink-1">{s.text}</span>
                    </div>
                  ))}
              {!parts.length && !steps.length && (
                <p className="py-8 text-center font-sans text-[13px] text-ink-3">Sin pasos registrados todavía.</p>
              )}
              {isWork(it) && (
                <div className="flex items-center gap-2 px-2 py-2 text-[12px] text-work">
                  <span className="inline-block size-2 rounded-full bg-work animate-sig-pulse" />
                  Ejecutando…
                </div>
              )}
            </div>
          </div>

          {!following && (
            <button
              type="button"
              onClick={() => {
                setFollowing(true)
                if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight
              }}
              className="pressable absolute right-3 bottom-3 inline-flex items-center gap-1 rounded-md border border-line-strong bg-elevated px-2.5 py-1 text-[12px] font-medium text-ink-1 shadow-[var(--shadow-md)] hover:bg-hover"
            >
              <ArrowDown className="size-3.5" />
              Seguir salida
            </button>
          )}
        </div>
      )}

      {/* Direct Composer Footer */}
      <footer className="flex items-center gap-2 border-t border-line bg-elevated px-3 py-2.5">
        <span className="font-mono text-[13px] text-ink-3" aria-hidden="true">
          &gt;
        </span>
        <input
          value={msg}
          onChange={(e) => setMsg(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault()
              send()
            }
          }}
          disabled={!sendable || busy === "send"}
          spellCheck={false}
          aria-label={`Mensaje a ${nameOf(it)}`}
          placeholder={
            sendable
              ? `Enviar instrucción a ${it.title}…`
              : it.paused
                ? "En pausa: reanúdala para escribirle"
                : it.kind === "agy"
                  ? "Se enlaza la primera vez que esta ventana corre un comando"
                  : "Solo las ventanas de Claude y Antigravity aceptan mensajes directos"
          }
          className="h-10 min-w-0 flex-1 bg-transparent text-[14px] text-ink-1 outline-none placeholder:text-ink-3 disabled:opacity-50"
        />
        <button
          type="button"
          disabled={!msg.trim() || !sendable || busy === "send"}
          onClick={send}
          aria-label="Enviar mensaje"
          className="pressable ag-grad-prime grid size-10 shrink-0 place-items-center rounded-lg disabled:opacity-35 disabled:pointer-events-none"
        >
          <ArrowUp className="size-4" />
        </button>
      </footer>
    </div>
  )
}

function ActionBtn({
  icon,
  label,
  why,
  onClick,
  busy,
  armed,
  danger,
  tip,
}: {
  icon: React.ReactNode
  label: string
  why?: string
  onClick: () => void
  busy?: boolean
  armed?: boolean
  danger?: boolean
  tip?: string
}) {
  return (
    <button
      type="button"
      disabled={!!why || busy}
      onClick={onClick}
      title={why || tip}
      className={cn(
        "pressable inline-flex h-8 items-center gap-1.5 rounded-lg border px-2.5 text-[12.5px] font-medium disabled:opacity-45",
        armed
          ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad"
          : danger
            ? "border-line-strong bg-surface text-ink-2 hover:border-[var(--sig-bad-border)] hover:text-bad"
            : "border-line-strong bg-surface text-ink-1 hover:bg-hover",
      )}
    >
      {icon}
      {label}
    </button>
  )
}
