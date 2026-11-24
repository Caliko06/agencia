import { useEffect, useRef, useState } from "react"
import { ArrowUp, RotateCcw, X } from "lucide-react"
import { Dialog } from "radix-ui"
import Markdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { post } from "@/lib/agencia"
import { buildDemoState } from "@/lib/demo"
import { up } from "@/lib/tone"
import { cn } from "@/lib/utils"

type Msg = { role: "user" | "assistant"; text: string; t: number; model?: string }
const MODELS = ["sonnet", "haiku"] as const

export function ConsultorDrawer({
  open,
  onOpenChange,
  demoMode,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  demoMode?: boolean
}) {
  const [msgs, setMsgs] = useState<Msg[]>([])
  const [v, setV] = useState("")
  const [model, setModel] = useState<(typeof MODELS)[number]>("sonnet")
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState("")
  const end = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open || busy || demoMode) return
    fetch("/api/consultor")
      .then((r) => r.json())
      .then((j) => setMsgs(j.msgs ?? []))
      .catch(() => setErr("La Agencia no contesta desde el servidor."))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, demoMode])

  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" })
  }, [msgs.length, busy])

  async function ask() {
    const text = v.trim()
    if (!text || busy) return
    const t = Date.now() / 1000
    setV("")
    setErr("")
    setBusy(true)
    setMsgs((p) => [...p, { role: "user", text, t }])
    if (demoMode) {
      setTimeout(() => {
        const demo = buildDemoState()
        const lines = demo.repos.map((r) => {
          const all = [
            ...r.windows,
            ...r.windows.flatMap((w) => w.children),
          ]
          const working = all.filter((x) => x.status === "working").length
          const waiting = all.filter((x) => x.status === "waiting" || x.status === "asks").length
          const failed = all.filter((x) => x.status === "failed").length
          const parts = [
            waiting ? `${waiting} pendiente${waiting > 1 ? "s" : ""}` : "",
            working ? `${working} trabajando` : "",
            failed ? `${failed} con fallo` : "",
          ].filter(Boolean)
          return `- \`${r.name}\`: ${parts.join(", ") || "sin bloqueos activos"}.`
        })
        setBusy(false)
        setMsgs((p) => [
          ...p,
          {
            role: "assistant",
            model,
            t: Date.now() / 1000,
            text: `En **Modo Demo**, el Consultor simula una lectura independiente del estado actual:\n\n${lines.join("\n")}`,
          },
        ])
      }, 450)
      return
    }
    const r = await post("/api/consultor", { text, model })
    setBusy(false)
    if (r.ok) setMsgs((p) => [...p, { role: "assistant", text: r.info ?? "", t: Date.now() / 1000, model }])
    else {
      setMsgs((p) => p.filter((m) => m.t !== t))
      setV(text)
      setErr(r.info || "No contestó.")
    }
  }

  async function reset() {
    if (busy) return
    setErr("")
    if (demoMode) {
      setMsgs([])
      return
    }
    if ((await post("/api/consultor", { reset: true })).ok) setMsgs([])
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="anim-overlay fixed inset-0 z-50 bg-black/55" />
        <Dialog.Content className="anim-drawer fixed top-0 right-0 bottom-0 z-50 flex w-[min(94vw,640px)] flex-col border-l border-line-strong bg-surface shadow-[var(--shadow-lg)]">
          <header className="flex flex-col gap-3 border-b border-line bg-elevated px-5 py-4">
            <div className="flex items-start justify-between gap-3">
              <div>
                <Dialog.Title className="text-[15px] font-semibold text-ink-1">Consultor independiente</Dialog.Title>
                <Dialog.Description className="mt-0.5 text-[12.5px] text-ink-3">
                  Lee el estado de la Agencia y tus repositorios sin modificar nada.
                </Dialog.Description>
              </div>
              <Dialog.Close asChild>
                <button
                  type="button"
                  aria-label="Cerrar consultor"
                  className="pressable grid size-7 place-items-center rounded-md border border-line bg-surface text-ink-3 hover:bg-hover hover:text-ink-1"
                >
                  <X className="size-4" />
                </button>
              </Dialog.Close>
            </div>
            <div className="flex items-center gap-2">
              <div className="inline-flex rounded-md border border-line bg-inset p-0.5" role="group" aria-label="Modelo">
                {MODELS.map((m) => (
                  <button
                    key={m}
                    type="button"
                    onClick={() => setModel(m)}
                    aria-pressed={model === m}
                    className={cn(
                      "pressable rounded px-2.5 py-1 text-[12px] font-medium",
                      model === m ? "bg-surface text-ink-1 shadow-[var(--shadow-sm)]" : "text-ink-3 hover:text-ink-1",
                    )}
                  >
                    {up(m)}
                  </button>
                ))}
              </div>
              <button
                type="button"
                onClick={reset}
                disabled={busy || !msgs.length}
                className="pressable ml-auto inline-flex h-7 items-center gap-1.5 rounded-md border border-line bg-surface px-2.5 text-[12px] text-ink-2 hover:bg-hover hover:text-ink-1"
              >
                <RotateCcw className="size-3" />
                Nueva charla
              </button>
            </div>
          </header>

          <div className="min-h-0 flex-1 overflow-y-auto p-5" role="log" aria-label="Conversación con el consultor">
            <div className="flex flex-col gap-3.5">
              {!msgs.length && !busy && (
                <p className="py-10 text-center text-[13px] text-ink-3">
                  Pregúntale si ya terminaron, qué riesgos detecta o qué frente conviene destrabar primero.
                </p>
              )}
              {msgs.map((m, i) =>
                m.role === "user" ? (
                  <div key={i} className="ml-auto max-w-[85%] rounded-lg border border-line-strong bg-elevated px-3.5 py-2.5 text-[13px] text-ink-1">
                    {m.text}
                  </div>
                ) : (
                  <div key={i} className="rounded-lg border border-line bg-inset p-3.5">
                    <div className="prose-ag">
                      <Markdown remarkPlugins={[remarkGfm]}>{m.text}</Markdown>
                    </div>
                    {m.model && <div className="mt-2 font-mono text-[10.5px] text-ink-3">{up(m.model)}</div>}
                  </div>
                ),
              )}
              {busy && (
                <div className="flex items-center gap-2 text-[12.5px] text-work" role="status">
                  <span className="size-2 rounded-full bg-work animate-sig-pulse" />
                  Leyendo estado y razonando…
                </div>
              )}
              {err && (
                <p className="text-[12.5px] text-bad" role="alert">
                  {err}
                </p>
              )}
              <div ref={end} />
            </div>
          </div>

          <div className="flex items-end gap-2 border-t border-line bg-elevated px-4 py-3">
            <textarea
              value={v}
              rows={2}
              autoFocus
              placeholder="Pregúntale al consultor…"
              aria-label="Pregunta"
              onChange={(e) => setV(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault()
                  ask()
                }
              }}
              className="min-w-0 flex-1 resize-none rounded-lg border border-line-strong bg-surface px-3 py-2 text-[13px] text-ink-1 outline-none placeholder:text-ink-3 focus:border-work"
            />
            <button
              type="button"
              onClick={ask}
              disabled={busy || !v.trim()}
              aria-label="Enviar"
              className="pressable grid size-9 shrink-0 place-items-center rounded-md bg-ink-1 text-ink-inv hover:opacity-90"
            >
              <ArrowUp className="size-4" />
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
