import type { Card, Win } from "@/lib/agencia"

export type Item = Card | Win
export type Tone = "need" | "bad" | "work" | "idle"

export const isWin = (x: Item): x is Win => "account" in x
/** Everything an item launched, at any depth: a Codex job's codexitos, the jobs a job started, their subagents. */
export const kidsOf = (x: Item): Card[] => x.children ?? []
export const flat = (w: Item): Item[] => [w, ...kidsOf(w).flatMap((c) => flat(c as Item))]
export const toneOf = (x: Item): Tone =>
  x.status === "asks" || x.status === "waiting"
    ? "need"
    : x.status === "failed"
      ? "bad"
      : x.status === "working"
        ? "work"
        : "idle"

export const TONE_WORD: Record<Tone, string> = {
  need: "Te espera",
  bad: "Falló",
  work: "Trabajando",
  idle: "Libre",
}

export const wordOf = (x: Item) => (x.status === "stalled" ? "Sin señal" : TONE_WORD[toneOf(x)])
export const nameOf = (x: Item) => (isWin(x) || x.kind === "sub" || x.kind === "bec" ? x.title : x.who)
export const subOf = (x: Item) =>
  isWin(x) ? x.cwd : x.kind === "sub" ? x.who : x.kind === "bec" ? becOf(x.model) : x.title
export const labelOf = (x: Item) =>
  isWin(x) || x.kind === "sub" || x.kind === "bec" ? x.title : `${x.who} — ${x.title}`
export const becOf = (m = "") => `Sub agente ${/glm/i.test(m) ? "GLM" : "DeepSeek"}`
export const base = (p: string) => p.replace(/\/+$/, "").split("/").pop() || p

const one = (s?: string) => (s || "").replace(/\s+/g, " ").trim()
export const up = (s: string) => (s ? s.charAt(0).toUpperCase() + s.slice(1) : "")

export const lineOf = (x: Item) =>
  x.status === "stalled"
    ? "Turno abierto sin avanzar. Quizá pide permiso en su terminal."
    : x.status === "asks" || x.status === "waiting"
      ? up(one(x.now) || "Te espera")
      : one(x.now) || (x.status === "working" ? "Trabajando" : one(x.request).slice(0, 140))

export const hhmm = (t: number) =>
  new Date(t * 1000).toLocaleTimeString("es", { hour: "2-digit", minute: "2-digit", hour12: false })

export function commonLead(lines: string[]): string {
  if (lines.length < 2 || lines.some((l) => !l)) return ""
  const w = lines.map((l) => l.split(" "))
  let n = 0
  while (w.every((x) => x[n] !== undefined && x[n] === w[0][n]) && w.every((x) => x.length > n + 1)) n++
  let lead = w[0].slice(0, n).join(" ")
  const cut = Math.max(lead.lastIndexOf(","), lead.lastIndexOf(":"), lead.lastIndexOf("·"))
  if (cut >= 12) lead = lead.slice(0, cut)
  return lead.length >= 12 ? lead : ""
}

export const afterLead = (s: string, lead: string) =>
  lead && s.startsWith(lead) ? s.slice(lead.length).replace(/^[\s,.:;·—-]+/, "") : s

export const modelOf = (m: string): string => {
  if (/codex-auto-review/.test(m)) return "Revisor de Codex"
  const id = (m || "").split("/").pop() ?? ""
  const cap = (x: string) => x.charAt(0).toUpperCase() + x.slice(1)
  let r = id.match(/^gpt-([\d.]+)-([a-z]+)/)
  if (r) return `${cap(r[2])} ${r[1]}`
  r = id.match(/^gpt-oss-([\w.]+)/i)
  if (r) return `GPT-OSS ${r[1].replace(/-(?:minimal|low|medium|high|xhigh)$/i, "").toUpperCase()}`
  r = id.match(/deepseek-v?([\d.]+)/)
  if (r) return `DeepSeek ${r[1]}`
  r = id.match(/^glm-([\d.]+)(?:-([a-z]+))?/i)
  if (r) return `GLM ${r[1]}${r[2] ? " " + cap(r[2]) : ""}`
  r = id.match(/claude-([a-z]+)-(\d+)-(\d+)/)
  if (r) return `${cap(r[1])} ${r[2]}.${r[3]}`
  r = id.match(/^gemini-([\d.]+)(?:-([a-z0-9]+))?/i)
  if (r) return `Gemini ${r[1]}${r[2] && !/^(?:minimal|low|medium|high|xhigh|tiered)$/i.test(r[2]) ? " " + cap(r[2]) : ""}`
  return id
}
