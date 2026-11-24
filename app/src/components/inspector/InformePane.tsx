import { useEffect, useMemo, useRef, useState, type ReactNode } from "react"
import {
  ChevronDown,
  FileDiff,
  FileText,
  GitCommitHorizontal,
  Image as ImageIcon,
  MessageSquareText,
  Pencil,
} from "lucide-react"
import Markdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { Shots } from "@/components/shots"
import { Viewer, urlOf } from "@/components/viewer"
import { useNow, type Part, type Recap, type Shot } from "@/lib/agencia"
import { hhmm, up } from "@/lib/tone"
import { cn } from "@/lib/utils"

const WRITE = new Set(["write", "edit", "multiedit", "notebookedit", "apply_patch"])
const READ = new Set(["read", "view"])
const SHELL = new Set(["bash", "shell", "exec", "exec_command", "run", "tool"])
const MD = /(?:~|\/|\.{1,2}\/)?[\w@.+/-]*[\w-]\.md\b/g

export type Ev = {
  kind: "commit" | "write" | "diff" | "read"
  t: number
  i: number
  label: string
  sub?: string
  path?: string
}

export interface Facts {
  evs: Ev[]
  t0: number
  t1: number
  steps: number
  asked: Part | null
  docs: { path: string; why: "escribió" | "citó" }[]
}

const clean = (s: string) => s.replace(/^["'`]|["'`,;]+$/g, "")
const mdIn = (s: string) => [...s.matchAll(MD)].map((m) => clean(m[0])).filter((p) => !p.includes("*") && p.length > 3)

export function factsOf(parts: Part[], reply: string): Facts {
  const evs: Ev[] = []
  const seen = new Set<string>()
  parts.forEach((p, i) => {
    if (p.kind !== "tool") return
    const n = (p.name ?? "").toLowerCase()
    const arg = p.arg ?? ""
    if (WRITE.has(n)) {
      for (const f of arg.split(/,\s+/).map((x) => x.trim()).filter(Boolean)) {
        const k = "w" + f
        if (!seen.has(k)) {
          seen.add(k)
          evs.push({ kind: "write", t: p.t, i, label: f.split("/").pop() || f, sub: f, path: f })
        }
      }
      return
    }
    if (READ.has(n)) {
      if (/\.md$/i.test(arg)) evs.push({ kind: "read", t: p.t, i, label: arg.split("/").pop() || arg, sub: arg, path: arg })
      return
    }
    if (!SHELL.has(n) && p.ok !== false && !/\bgit\b/.test(arg)) return
    if (/\bgit\b[^|;&\n]*\bcommit\b/.test(arg) && p.ok !== false) {
      const m = (p.out ?? "").match(/\[([^\]\s]+)(?: \(root-commit\))? ([0-9a-f]{7,40})\]\s*(.+)/)
      const msg = m?.[3] ?? arg.match(/-m\s+["']([^"']+)/)?.[1] ?? "commit"
      evs.push({ kind: "commit", t: p.t, i, label: msg, sub: m ? `${m[1]} · ${m[2]}` : "" })
      return
    }
    if (/\bgit\b[^|;&\n]*\b(diff|show)\b|\bgit\b[^|;&\n]*\blog\b[^|;&\n]*\s-p\b/.test(arg)) {
      const what = arg.replace(/^.*?\bgit\b\s+(?:-C\s+\S+\s+)?/, "git ").replace(/\s*\|.*$/, "")
      evs.push({ kind: "diff", t: p.t, i, label: what.length > 90 ? what.slice(0, 89) + "…" : what, sub: "" })
      return
    }
    const md = arg.match(/\b(?:cat|head|tail|sed|less|bat|nl)\b[^|;&\n]*?([~/\w.@+-]+\.md)\b/)
    if (md) evs.push({ kind: "read", t: p.t, i, label: md[1].split("/").pop() || md[1], sub: md[1], path: md[1] })
  })

  const docs: Facts["docs"] = []
  for (const e of [...evs].reverse()) {
    if (e.kind === "write" && e.path && /\.md$/i.test(e.path) && !docs.some((d) => d.path === e.path)) {
      docs.push({ path: e.path, why: "escribió" })
    }
  }
  const known = (f: string) =>
    docs.some((d) => d.path === f || d.path.split("/").pop() === f.split("/").pop()) ||
    evs.some((e) => e.kind === "read" && e.path?.split("/").pop() === f.split("/").pop())
  for (const f of mdIn(reply)) if (!known(f)) docs.push({ path: f, why: "citó" })

  const ts = parts.map((p) => p.t).filter(Boolean)
  return {
    evs,
    t0: ts.length ? Math.min(...ts) : 0,
    t1: ts.length ? Math.max(...ts) : 0,
    steps: parts.filter((p) => p.kind === "tool").length,
    asked: [...parts].reverse().find((p) => p.role === "user" && p.kind === "text") ?? null,
    docs,
  }
}

export const dur = (s: number) =>
  s >= 600 && s < 3600
    ? `${Math.round(s / 60)} min`
    : s < 60
      ? `${Math.max(1, Math.round(s))} s`
      : s < 3600
        ? `${Math.floor(s / 60)} min${Math.round(s % 60) ? ` ${Math.round(s % 60)} s` : ""}`
        : `${Math.floor(s / 3600)} h ${Math.round((s % 3600) / 60)} min`

export function sayOf(reply: string): { lead: string; body: string } {
  let t = reply.trim()
  const i = t.search(/^RESUMEN:\s*/m)
  if (i >= 0) t = t.slice(i)
  const m = t.match(/^RESUMEN:\s*([\s\S]*?)(?=\n\s*\n|\n[A-ZÁÉÍÓÚÑ][A-ZÁÉÍÓÚÑ ]{2,}:[ \t]*(?:\n|$)|\n\s*(?:[-*]|\d+\.)\s|$)/)
  const lead = m ? m[1].trim() : ""
  const body = (m ? t.slice(m[0].length) : t).trim()
  return {
    lead,
    body: body.replace(/^([A-ZÁÉÍÓÚÑ][A-ZÁÉÍÓÚÑ ]{2,}):\s*$/gm, (_, h: string) => `#### ${up(h.toLowerCase())}`),
  }
}

const ICON = { commit: GitCommitHorizontal, write: Pencil, diff: FileDiff, read: FileText } as const
const TITLE = { commit: "Commits", write: "Archivos escritos", diff: "Diffs revisados", read: "Documentos leídos" } as const

function Md({ text, cwd }: { text: string; cwd?: string }) {
  return (
    <div className="prose-ag">
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <span className="underline decoration-line-strong" title={href}>
              {children}
            </span>
          ),
          code: ({ children, className }) => {
            const s = String(children)
            return !className && !s.includes("\n") && /^[~\w./@+-]+\.md$/.test(s) ? (
              <Viewer src={s} cwd={cwd} className="inline underline decoration-work/60 hover:text-work">
                <code>{s}</code>
              </Viewer>
            ) : (
              <code className={className}>{children}</code>
            )
          },
        }}
      >
        {text}
      </Markdown>
    </div>
  )
}

function Fold({ children, tall = 170 }: { children: ReactNode; tall?: number }) {
  const [open, setOpen] = useState(false)
  const [big, setBig] = useState(false)
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = bodyRef.current
    if (!el) return
    const check = () => {
      const b = el.scrollHeight > tall + 24
      setBig((prev) => (prev === b ? prev : b))
    }
    check()
    const ro = new ResizeObserver(check)
    ro.observe(el)
    if (el.firstElementChild) ro.observe(el.firstElementChild)
    return () => ro.disconnect()
  }, [tall, children])

  return (
    <div>
      <div
        ref={bodyRef}
        className="overflow-hidden"
        style={{ maxHeight: open ? undefined : tall }}
      >
        {children}
      </div>
      {big && (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          className="mt-1.5 inline-flex items-center gap-1 text-[12.5px] font-medium text-ink-2 hover:text-ink-1"
        >
          <ChevronDown className={cn("size-3.5 transition-transform duration-150", open && "rotate-180")} />
          {open ? "Mostrar menos" : "Leer todo"}
        </button>
      )}
    </div>
  )
}

function EmbedDoc({ docs, cwd }: { docs: Facts["docs"]; cwd?: string }) {
  const [n, setN] = useState(0)
  const [t, setT] = useState<string | null>(null)
  const d = docs[n]
  useEffect(() => {
    if (!d) return
    let on = true
    setT(null)
    fetch(urlOf(d.path, cwd))
      .then(async (r) => {
        const b = await r.text()
        if (!on) return
        if (r.ok) setT(b)
        else setN((x) => x + 1)
      })
      .catch(() => on && setN((x) => x + 1))
    return () => {
      on = false
    }
  }, [d, cwd])

  if (!d) return null
  return (
    <section className="flex flex-col gap-2 border-t border-line pt-4">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-[13px] font-semibold text-ink-1">Documento que {d.why}</h3>
        <Viewer src={d.path} cwd={cwd} className="text-[12.5px] font-medium text-work hover:underline">
          Abrir completo
        </Viewer>
      </div>
      <p className="truncate font-mono text-[12px] text-ink-3" title={d.path}>
        {d.path}
      </p>
      <div className="rounded-lg border border-line bg-inset p-3">
        {t === null ? (
          <p className="text-[12.5px] text-ink-3">Cargando…</p>
        ) : (
          <Fold tall={240}>
            <Md text={t} cwd={cwd} />
          </Fold>
        )}
      </div>
    </section>
  )
}

export function InformePane({
  parts,
  reply,
  request,
  recap,
  shots,
  name,
  cwd,
  updated,
  working,
  you,
  cap,
  jump,
}: {
  parts: Part[]
  reply: string
  request: string
  recap?: Recap | null
  shots?: Shot[]
  name: string
  cwd?: string
  updated: number
  working: boolean
  you: boolean
  cap: boolean
  jump: (i: number) => void
}) {
  const now = useNow()
  const f = useMemo(() => factsOf(parts, reply), [parts, reply])
  const lastSay = [...parts].reverse().find((p) => p.role === "assistant" && p.kind === "text")?.text ?? ""
  const say = useMemo(() => sayOf(reply || lastSay), [reply, lastSay])
  const groups = (["commit", "write", "diff", "read"] as const)
    .map((k) => [k, f.evs.filter((e) => e.kind === k)] as const)
    .filter(([, l]) => l.length > 0)
  const nShots = shots?.length ?? 0
  const asked = you ? f.asked?.text || request : request || f.asked?.text
  const askedT = you ? f.asked?.t ?? 0 : parts[0]?.t ?? 0
  const none = !groups.length && !nShots

  const counts = [
    ["commit", "commit", "commits"],
    ["write", "archivo", "archivos"],
    ["diff", "diff", "diffs"],
    ["read", ".md leído", ".md leídos"],
  ] as const

  return (
    <div className="flex flex-col gap-4 p-4">
      {/* Telemetry Summary Strip */}
      <section className="rounded-lg border border-line bg-inset p-3">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[12.5px] text-ink-2">
          <span className="font-semibold text-ink-1">
            {working ? "Trabajando" : "Última señal"}
            {!working && updated ? ` hace ${dur(now - updated)}` : ""}
          </span>
          {f.t0 > 0 && f.t1 > f.t0 && (
            <span className="font-mono text-[12px] tabular-nums text-ink-3">
              {cap ? "últimos " : "duró "}
              {dur(f.t1 - f.t0)}
            </span>
          )}
          {f.steps > 0 && (
            <span className="font-mono text-[12px] tabular-nums text-ink-3">
              {f.steps} {f.steps === 1 ? "paso" : "pasos"}
              {cap ? " (tope 80)" : ""}
            </span>
          )}
        </div>
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {counts.map(([k, one, many]) => {
            const n = f.evs.filter((e) => e.kind === k).length
            if (!n) return null
            const Icon = ICON[k]
            return (
              <span
                key={k}
                className="inline-flex items-center gap-1.5 rounded border border-line bg-surface px-2 py-0.5 text-[12px] text-ink-2"
              >
                <Icon className="size-3 text-ink-3" />
                <b className="font-mono font-semibold tabular-nums text-ink-1">{n}</b>
                {n === 1 ? one : many}
              </span>
            )
          })}
          {nShots > 0 && (
            <span className="inline-flex items-center gap-1.5 rounded border border-line bg-surface px-2 py-0.5 text-[12px] text-ink-2">
              <ImageIcon className="size-3 text-ink-3" />
              <b className="font-mono font-semibold tabular-nums text-ink-1">{nShots}</b>
              {nShots === 1 ? "captura" : "capturas"}
            </span>
          )}
          {none && (
            <span className="text-[12px] text-ink-3">
              Sin archivos, diffs ni commits{cap ? " en los últimos 80 pasos" : ""}
            </span>
          )}
        </div>
      </section>

      {/* Agent Statement */}
      {(say.lead || say.body || recap?.did) && (
        <section className="flex flex-col gap-2">
          <div className="flex items-baseline justify-between">
            <h3 className="text-[13px] font-semibold text-ink-1">
              {working ? "Lo último que dijo" : "Resumen del agente"}
            </h3>
            {f.t1 > 0 && <span className="font-mono text-[12px] tabular-nums text-ink-3">{hhmm(f.t1)}</span>}
          </div>
          {recap?.did && (
            <div className="flex flex-col gap-1 rounded-lg border-l-2 border-work bg-[var(--sig-work-soft)] px-3 py-2 text-[13px]">
              <p className="text-ink-1">
                <span className="mr-2 font-mono text-[12px] font-semibold text-work">Hizo</span>
                {recap.did}
              </p>
              {recap.next && (
                <p className="text-ink-2">
                  <span className="mr-2 font-mono text-[12px] font-semibold text-ink-3">Sigue</span>
                  {recap.next}
                </p>
              )}
            </div>
          )}
          {say.lead && (
            <div className="rounded-lg border border-line bg-elevated p-3">
              <Fold tall={180}>
                <Md text={say.lead} cwd={cwd} />
              </Fold>
            </div>
          )}
          {say.body && (
            <div className="px-0.5">
              <Fold>
                <Md text={say.body} cwd={cwd} />
              </Fold>
            </div>
          )}
        </section>
      )}

      {/* Evidence */}
      <section className="flex flex-col gap-2.5 border-t border-line pt-4">
        <h3 className="text-[13px] font-semibold text-ink-1">Evidencia verificable</h3>
        {nShots > 0 && (
          <div className="flex flex-wrap gap-2">
            <Shots shots={shots} name={name} strip />
          </div>
        )}
        {groups.map(([k, list]) => {
          const Icon = ICON[k]
          return (
            <div key={k} className="flex flex-col gap-1">
              <h4 className="text-[12px] font-medium text-ink-3">{TITLE[k]}</h4>
              <ul className="divide-y divide-line-subtle overflow-hidden rounded-lg border border-line bg-inset">
                {list
                  .slice(-6)
                  .reverse()
                  .map((e) => {
                    const dir =
                      e.sub && e.sub !== e.label
                        ? e.sub.slice(0, Math.max(0, e.sub.length - e.label.length)).replace(/\/$/, "")
                        : ""
                    return (
                      <li key={`${k}${e.i}${e.label}`} className="flex items-center gap-2 px-2.5 py-1.5 text-[12.5px]">
                        <Icon className="size-3.5 shrink-0 text-ink-3" />
                        {e.path ? (
                          <Viewer
                            src={e.path}
                            cwd={cwd}
                            title={e.sub || e.label}
                            className="flex min-w-0 flex-1 items-baseline gap-1.5 text-left font-mono text-[12px] hover:text-work"
                          >
                            <span className="max-w-full shrink-0 truncate font-medium text-ink-1">{e.label}</span>
                            {dir && <span className="min-w-0 truncate text-ink-3">{dir}</span>}
                          </Viewer>
                        ) : (
                          <button
                            type="button"
                            onClick={() => jump(e.i)}
                            title="Ver en la terminal"
                            className="min-w-0 flex-1 truncate text-left text-ink-1 hover:text-work"
                          >
                            <span className={cn(k === "diff" && "font-mono text-[12px]")}>{e.label}</span>
                          </button>
                        )}
                        <span className="shrink-0 font-mono text-[12px] tabular-nums text-ink-3">{hhmm(e.t)}</span>
                      </li>
                    )
                  })}
              </ul>
              {list.length > 6 && <p className="text-[12px] text-ink-3">y {list.length - 6} más, más antiguos</p>}
            </div>
          )
        })}
        {none && (
          <p className="text-[12.5px] text-ink-3">
            No dejó archivos escritos, diffs, commits ni capturas{cap ? " en sus últimos 80 pasos" : ""}.
          </p>
        )}
      </section>

      {f.docs.length > 0 && <EmbedDoc key={f.docs.map((d) => d.path).join()} docs={f.docs} cwd={cwd} />}

      {asked && (
        <section className="flex flex-col gap-1.5 border-t border-line pt-4">
          <div className="flex items-baseline justify-between">
            <h3 className="text-[13px] font-semibold text-ink-1">
              {you ? "Última instrucción enviada" : "Pedido asignado"}
            </h3>
            {askedT > 0 && <span className="font-mono text-[12px] tabular-nums text-ink-3">{hhmm(askedT)}</span>}
          </div>
          <Fold tall={96}>
            <div className="flex items-start gap-2 rounded-lg border border-line bg-inset p-3 text-[13px] text-ink-2">
              <MessageSquareText className="mt-0.5 size-3.5 shrink-0 text-ink-3" />
              <p className="whitespace-pre-wrap">{asked}</p>
            </div>
          </Fold>
        </section>
      )}
    </div>
  )
}
