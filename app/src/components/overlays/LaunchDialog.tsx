import { useEffect, useMemo, useState } from "react"
import { ArrowRight, Loader2, X } from "lucide-react"
import { Dialog } from "radix-ui"
import { toast } from "sonner"
import { Brand } from "@/components/brand"
import { post, type Kind, type Quota } from "@/lib/agencia"
import { base } from "@/lib/tone"
import { cn } from "@/lib/utils"

type LaunchKind = "cdx" | "cla" | "bec"
type RuntimeOption = {
  key: string
  kind: LaunchKind
  home: string
  brand: Kind
  label: string
  hint: string
}

const brandForLaunchKind = (k: LaunchKind): Kind => (k === "cla" ? "claude" : k === "bec" ? "bec" : "codex")

function buildRuntimeOptions(
  homes?: { kind: string; id: string; label: string; desc?: string }[],
  quota?: Quota,
): RuntimeOption[] {
  const opts: RuntimeOption[] = []
  const seen = new Set<string>()
  const push = (opt: RuntimeOption) => {
    if (seen.has(opt.key)) return
    seen.add(opt.key)
    opts.push(opt)
  }

  if (homes?.length) {
    for (const h of homes) {
      const k: LaunchKind = h.kind === "cla" || h.kind === "claude" ? "cla" : h.kind === "bec" ? "bec" : "cdx"
      const home = k === "bec" ? "" : h.id
      push({
        key: `${k}:${home}`,
        kind: k,
        home,
        brand: brandForLaunchKind(k),
        label: h.label,
        hint: h.desc || (k === "cdx" ? "Hilo retomable" : k === "cla" ? "Worker en repo" : "Cola limpia"),
      })
    }
  } else if (quota?.codex?.length) {
    for (const m of quota.codex) {
      const id = /^codex$/i.test(m.name.trim())
        ? "default"
        : m.name.toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-|-$/g, "") || "default"
      push({
        key: `cdx:${id}`,
        kind: "cdx",
        home: id,
        brand: "codex",
        label: /^codex/i.test(m.name) ? m.name : `Codex ${m.name}`,
        hint: "Hilo retomable",
      })
    }
  }

  if (!opts.some((o) => o.kind === "cdx")) {
    push({
      key: "cdx:default",
      kind: "cdx",
      home: "default",
      brand: "codex",
      label: "Codex",
      hint: "Hilo retomable",
    })
  }
  if (!opts.some((o) => o.kind === "cla")) {
    push({
      key: "cla:",
      kind: "cla",
      home: "",
      brand: "claude",
      label: "Claude",
      hint: "Worker en repo",
    })
  }
  if (!opts.some((o) => o.kind === "bec")) {
    push({
      key: "bec:",
      kind: "bec",
      home: "",
      brand: "bec",
      label: "DeepSeek",
      hint: "Cola limpia",
    })
  }

  return opts
}

export function LaunchDialog({
  open,
  onOpenChange,
  cwd,
  demoMode,
  homes,
  quota,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  cwd: string
  demoMode?: boolean
  homes?: { kind: string; id: string; label: string; desc?: string }[]
  quota?: Quota
}) {
  const runtimeOptions = useMemo(() => buildRuntimeOptions(homes, quota), [homes, quota])
  const defaultKey = useMemo(
    () => runtimeOptions.find((o) => o.kind === "cdx" && o.home === "default")?.key ?? runtimeOptions[0]?.key ?? "cdx:default",
    [runtimeOptions],
  )
  const [tui, setTui] = useState(true)
  const [selectedKey, setSelectedKey] = useState<string>(defaultKey)
  const [customRepo, setCustomRepo] = useState<string | null>(null)
  const [text, setText] = useState("")
  const [busy, setBusy] = useState(false)
  const [fetchedRepos, setFetchedRepos] = useState<{ name: string; path: string }[]>([])

  const repo = customRepo ?? cwd
  const activeOpt = runtimeOptions.find((o) => o.key === selectedKey) ?? runtimeOptions[0]
  const repos = useMemo(
    () =>
      demoMode
        ? [
            { name: "rust-gateway", path: "~/workspace/rust-gateway" },
            { name: "log-indexer", path: "~/workspace/log-indexer" },
            { name: "demo-engine", path: "~/workspace/demo-engine" },
          ]
        : fetchedRepos,
    [demoMode, fetchedRepos],
  )

  useEffect(() => {
    if (!open || demoMode) return
    let dead = false
    fetch("/api/repos")
      .then((r) => r.json())
      .then((list) => {
        if (!dead && Array.isArray(list)) setFetchedRepos(list)
      })
      .catch(() => {
        if (!dead) setFetchedRepos([])
      })
    return () => {
      dead = true
    }
  }, [open, demoMode])

  const tilde = (p: string) => p.replace(/^\/Users\/[^/]+/, "~")
  const options = repos.some((r) => tilde(r.path) === tilde(cwd)) ? repos : [{ name: base(cwd), path: cwd }, ...repos]
  const value = options.find((r) => tilde(r.path) === tilde(repo))?.path ?? options[0]?.path ?? cwd

  async function submit() {
    if (!text.trim() || busy || !activeOpt) return
    if (demoMode) {
      toast.success("Agente lanzado (simulado en Demo)")
      setText("")
      setCustomRepo(null)
      onOpenChange(false)
      return
    }
    setBusy(true)
    const r = await post("/api/launch", {
      tui: activeOpt.kind === "cdx" && tui,
      kind: activeOpt.kind,
      home: activeOpt.home,
      repo: value || repo,
      text:
        activeOpt.kind === "bec" && !/EN UNA L[IÍ]NEA/i.test(text)
          ? `${text}\nEN UNA LÍNEA: ${text.split("\n")[0].slice(0, 90)}`
          : text,
    })
    setBusy(false)
    if (r.ok) {
      toast.success("Trabajo lanzado", { description: r.id ? `ID ${r.id}` : "Abierto en Terminal" })
      setText("")
      setCustomRepo(null)
      onOpenChange(false)
    } else {
      toast.error("No se pudo lanzar", { description: r.info })
    }
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) setCustomRepo(null)
        onOpenChange(next)
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="anim-overlay fixed inset-0 z-50 bg-black/60" />
        <Dialog.Content
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault()
              submit()
            }
          }}
          className="anim-modal fixed top-1/2 left-1/2 z-50 w-[min(92vw,600px)] -translate-x-1/2 -translate-y-1/2 overflow-hidden rounded-xl border border-line-strong bg-surface shadow-[var(--shadow-lg)]"
        >
          <div className="flex items-start justify-between gap-3 border-b border-line bg-elevated px-5 py-4">
            <div className="min-w-0 flex-1">
              <Dialog.Title className="text-[15px] font-semibold text-ink-1">
                Lanzar agente en <span className="font-mono text-work">{base(value || repo)}</span>
              </Dialog.Title>
              <Dialog.Description
                title={`${value || repo} · contexto limpio, aparece al instante`}
                className="mt-0.5 truncate font-mono text-[11.5px] text-ink-3"
              >
                {value || repo} · contexto limpio, aparece al instante
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <button
                type="button"
                aria-label="Cerrar"
                className="pressable grid size-7 shrink-0 place-items-center rounded-md border border-line bg-surface text-ink-3 hover:bg-hover hover:text-ink-1"
              >
                <X className="size-4" />
              </button>
            </Dialog.Close>
          </div>

          <div className="flex flex-col gap-4 p-5">
            <textarea
              autoFocus
              rows={5}
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="Describe el objetivo concreto para este agente…"
              aria-label="Pedido"
              className="w-full resize-none rounded-lg border border-line-strong bg-inset p-3.5 text-[13.5px] leading-relaxed text-ink-1 outline-none placeholder:text-ink-3 focus:border-work"
            />

            <div>
              <div className="mb-1.5 text-[12px] font-medium text-ink-2">Runtime</div>
              <div className="grid grid-cols-[repeat(auto-fit,minmax(145px,1fr))] gap-1.5">
                {runtimeOptions.map((opt) => (
                  <button
                    key={opt.key}
                    type="button"
                    onClick={() => setSelectedKey(opt.key)}
                    aria-pressed={activeOpt?.key === opt.key}
                    className={cn(
                      "pressable flex flex-col items-start gap-0.5 rounded-lg border px-3 py-2 text-left",
                      activeOpt?.key === opt.key
                        ? "border-ink-1 bg-elevated text-ink-1"
                        : "border-line bg-inset text-ink-2 hover:bg-hover",
                    )}
                  >
                    <span className="flex items-center gap-1.5 text-[12.5px] font-semibold">
                      <Brand kind={opt.brand} className="size-3.5" />
                      {opt.label}
                    </span>
                    <span className="text-[11px] text-ink-3">{opt.hint}</span>
                  </button>
                ))}
              </div>
            </div>

            {activeOpt?.kind === "cdx" && (
              <div className="grid grid-cols-2 gap-1.5">
                {([
                  [true, "Ventana real (TUI)", "Se abre en Terminal para verlo y escribirle"],
                  [false, "Segundo plano", "Sin ventana; se puede abrir al terminar"],
                ] as const).map(([val, label, hint]) => (
                  <button
                    key={String(val)}
                    type="button"
                    onClick={() => setTui(val)}
                    aria-pressed={tui === val}
                    className={cn(
                      "pressable flex flex-col items-start gap-0.5 rounded-lg border px-3 py-2 text-left",
                      tui === val ? "border-ink-1 bg-elevated text-ink-1" : "border-line bg-inset text-ink-2 hover:bg-hover",
                    )}
                  >
                    <span className="text-[12.5px] font-semibold">{label}</span>
                    <span className="text-[11px] text-ink-3">{hint}</span>
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="flex items-center justify-between gap-3 border-t border-line bg-elevated px-5 py-3.5">
            <select
              value={value}
              onChange={(e) => setCustomRepo(e.target.value)}
              aria-label="Repositorio destino"
              className="h-8 min-w-0 flex-1 rounded-md border border-line-strong bg-surface px-2.5 text-[12.5px] text-ink-1 outline-none sm:max-w-[260px]"
            >
              {options.map((r) => (
                <option key={r.path} value={r.path}>
                  {r.name}
                </option>
              ))}
            </select>
            <button
              type="button"
              disabled={!text.trim() || busy}
              onClick={submit}
              className="pressable inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md bg-ink-1 px-3.5 text-[12.5px] font-semibold text-ink-inv hover:opacity-90"
            >
              {busy ? <Loader2 className="size-3.5 animate-spin" /> : <ArrowRight className="size-3.5" />}
              Lanzar
              <kbd className="ml-1 rounded border border-current/25 px-1 font-mono text-[10px]">⌘↵</kbd>
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
