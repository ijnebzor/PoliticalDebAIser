import React, { useMemo, useState } from "react";
import { motion } from "framer-motion";

/**
 * Debiaser Webapp – Starter UI (Enhanced per request)
 * Adds:
 * - Economic vs Social axes with legend + colour scale
 * - Persona clustering by agreement on Liberty–Order axis
 * - Gradient banding per cluster and ordered personas
 * - Disagreement meter with cluster sizes in the caption
 */

// ------------------ Types ------------------
export type PersonaId =
  | "progressive_activist"
  | "liberal_social_democrat"
  | "centrist_technocrat"
  | "libertarian_civil"
  | "conservative_fiscal"
  | "national_security_hawk"
  | "environmentalist_green"
  | "populist_anti_elite";

export interface PersonaOutput {
  id: PersonaId;
  title: string;
  stance_score: number; // -3 = liberty, 0 = centre, +3 = order
  confidence: number; // 0..1
  summary: string; // 2–4 sentences
  key_claims: string[];
  fact_checks: {
    claim: string;
    assessment: "supported" | "contested" | "unsupported" | "unclear";
    rationale: string;
  }[];
  caveats: string[];
  // Optional extra axes for 2D view
  axes?: { economic: number; social: number }; // -3..+3 each
}

export interface DebiasedSummary {
  consensus_points: string[];
  disagreements: string[];
  likely_bias_drivers: string[];
  truth_seeking_summary: string;
  spectrum_score: number; // -3..+3
  spectrum_explain: string;
}

export interface AnalysisResult {
  title: string;
  source_url?: string;
  personas: PersonaOutput[];
  debiaser: DebiasedSummary;
}

// ------------------ Mock pipeline ------------------
async function mockAnalyse(input: { url?: string; text?: string }): Promise<AnalysisResult> {
  const personas: PersonaOutput[] = [
    {
      id: "progressive_activist",
      title: "Progressive Activist",
      stance_score: -2.2,
      confidence: 0.78,
      summary:
        "Emphasises civil rights and disproportionate impacts. Warns of chilling effects on speech.",
      key_claims: ["Oversurveillance harms minorities", "Deterrence claims are speculative", "Independent oversight is missing"],
      fact_checks: [{ claim: "Surveillance reduces crime", assessment: "contested", rationale: "Mixed evidence across contexts." }],
      caveats: ["May underweight short-term security benefits"],
      axes: { economic: -1.2, social: -2.0 },
    },
    {
      id: "liberal_social_democrat",
      title: "Liberal Social Democrat",
      stance_score: -1.2,
      confidence: 0.73,
      summary:
        "Supports targeted measures with safeguards. Calls for proportionality tests and data minimisation.",
      key_claims: ["Warrants and audits", "Proportionality not shown"],
      fact_checks: [{ claim: "Strong oversight exists", assessment: "unsupported", rationale: "No referenced audits." }],
      caveats: ["Assumes institutions can deliver robust safeguards"],
      axes: { economic: -0.8, social: -0.6 },
    },
    {
      id: "centrist_technocrat",
      title: "Centrist Technocrat",
      stance_score: 0.1,
      confidence: 0.8,
      summary:
        "Seeks measurable outcomes. Requests KPIs, error rates, cost–benefit, and sunset clauses.",
      key_claims: ["KPIs missing", "Pilot then evaluate", "Sunset clauses"],
      fact_checks: [{ claim: "Costs are modest", assessment: "unclear", rationale: "No transparent TCO provided." }],
      caveats: ["May appear aloof to rights framing"],
      axes: { economic: 0.0, social: 0.0 },
    },
    {
      id: "libertarian_civil",
      title: "Libertarian, Civil Liberties",
      stance_score: -2.6,
      confidence: 0.76,
      summary:
        "Frames privacy as a fundamental liberty. Warns of mission creep and power asymmetry.",
      key_claims: ["Mission creep risk", "Consent absent", "Power asymmetry"],
      fact_checks: [{ claim: "Only criminals are affected", assessment: "unsupported", rationale: "Dragnet approaches catch innocents." }],
      caveats: ["Can underweight collective benefits"],
      axes: { economic: 0.4, social: -2.4 },
    },
    {
      id: "conservative_fiscal",
      title: "Conservative, Fiscal",
      stance_score: 1.4,
      confidence: 0.7,
      summary:
        "Prioritises order and costs. Supports surveillance if efficient, legal, and with penalties for abuse.",
      key_claims: ["Cost discipline", "Law-and-order", "Penalties for misuse"],
      fact_checks: [{ claim: "Deterrence proven", assessment: "contested", rationale: "Evidence varies by crime type." }],
      caveats: ["May underweight minority-rights concerns"],
      axes: { economic: 1.6, social: 1.2 },
    },
    {
      id: "national_security_hawk",
      title: "National Security Hawk",
      stance_score: 2.2,
      confidence: 0.74,
      summary:
        "Focuses on threat landscape. Favors tools that close intelligence gaps with internal compliance.",
      key_claims: ["High-impact threats", "Intelligence gaps", "Rapid response"],
      fact_checks: [{ claim: "Transparency always feasible", assessment: "contested", rationale: "Operational secrecy sometimes required." }],
      caveats: ["Risks normalising exceptional powers"],
      axes: { economic: 0.8, social: 2.0 },
    },
    {
      id: "environmentalist_green",
      title: "Environmentalist Green",
      stance_score: -0.8,
      confidence: 0.69,
      summary:
        "Flags energy use and supply-chain risks. Notes activism chill in a climate of fear.",
      key_claims: ["Energy footprint", "Activism chill", "Sourcing risks"],
      fact_checks: [{ claim: "Impact negligible", assessment: "unsupported", rationale: "No lifecycle numbers provided." }],
      caveats: ["May over-ascribe climate lens"],
      axes: { economic: -1.4, social: -0.4 },
    },
    {
      id: "populist_anti_elite",
      title: "Populist, Anti-elite",
      stance_score: 1.0,
      confidence: 0.62,
      summary:
        "Suspicious of elites and tech firms. Accepts measures if aimed at corrupt insiders, not ordinary citizens.",
      key_claims: ["Elites exempt?", "Corporate capture", "Equal application"],
      fact_checks: [{ claim: "Benefits the public", assessment: "unclear", rationale: "Who benefits is unspecified." }],
      caveats: ["Highly contingent and rhetorical"],
      axes: { economic: -0.2, social: 1.6 },
    },
  ];

  const spectrum = weightedMean(personas.map((p) => ({ score: p.stance_score, weight: p.confidence })));

  const debiaser: DebiasedSummary = {
    consensus_points: [
      "Evidence for deterrence is mixed and context-dependent",
      "Oversight, audits, and guardrails materially change risk",
      "Costs, energy use, and supply chains need transparency",
    ],
    disagreements: [
      "Relative weight of liberty versus safety",
      "Acceptable level of secrecy for operations",
      "How to measure impact and proportionality",
    ],
    likely_bias_drivers: [
      "Security-first framing that privileges order over rights",
      "Assumptions that technology is net-positive without audits",
      "Understated risks of mission creep and disparate impact",
    ],
    truth_seeking_summary:
      "On balance, the article advances a security-first position. It cites recent threats, but lacks robust metrics, guardrails, and independent oversight details. A cautious reader should ask for proportionality tests, pilot evidence, audits, and time-limited authorisations.",
    spectrum_score: Number(spectrum.toFixed(2)),
    spectrum_explain:
      "Placement reflects persona-weighted views on a Liberty–Order axis.",
  };

  return { title: "Sample: Surveillance as Public Safety Tool", source_url: input.url, personas, debiaser };
}

function weightedMean(items: { score: number; weight: number }[]) {
  const n = items.reduce((a, b) => a + b.score * b.weight, 0);
  const d = items.reduce((a, b) => a + b.weight, 0) || 1;
  return n / d;
}

function stdDev(items: { score: number; weight: number }[]) {
  const mean = weightedMean(items);
  const wSum = items.reduce((a, b) => a + b.weight, 0) || 1;
  const variance = items.reduce((acc, it) => acc + it.weight * Math.pow(it.score - mean, 2), 0) / wSum;
  return Math.sqrt(variance);
}

function clusterByAgreement(sorted: PersonaOutput[], gap = 0.9) {
  // sorted by stance_score asc; start new cluster when gap exceeds threshold
  const clusters: PersonaOutput[][] = [];
  let current: PersonaOutput[] = [];
  for (let i = 0; i < sorted.length; i++) {
    if (i === 0) {
      current.push(sorted[i]);
    } else {
      const prev = sorted[i - 1];
      if (Math.abs(sorted[i].stance_score - prev.stance_score) > gap) {
        clusters.push(current);
        current = [sorted[i]];
      } else {
        current.push(sorted[i]);
      }
    }
  }
  if (current.length) clusters.push(current);
  return clusters;
}

function colourForAxes(economic: number, social: number) {
  // Map economic (x) to hue, social (y) to lightness. Hue: -3..+3 -> 220..340, lightness: 35..60
  const clamp = (v: number) => Math.max(-3, Math.min(3, v));
  const h = 220 + ((clamp(economic) + 3) / 6) * 120; // blue -> magenta
  const l = 60 - ((clamp(social) + 3) / 6) * 25; // more authoritarian -> darker
  return `hsl(${h.toFixed(0)}, 70%, ${l.toFixed(0)}%)`;
}

// ------------------ UI ------------------
export default function App() {
  const [url, setUrl] = useState("");
  const [text, setText] = useState("");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<AnalysisResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [show2D, setShow2D] = useState(false);

  const spectrum = result?.debiaser.spectrum_score ?? 0;

  async function onAnalyse() {
    setLoading(true);
    setError(null);
    try {
      const data = await mockAnalyse({ url: url || undefined, text: text || undefined });
      setResult(data);
    } catch (e: any) {
      setError(e?.message || "Something went wrong.");
    } finally {
      setLoading(false);
    }
  }

  const personasSorted = useMemo(() => (result ? [...result.personas].sort((a, b) => a.stance_score - b.stance_score) : []), [result]);
  const clusters = useMemo(() => clusterByAgreement(personasSorted), [personasSorted]);
  const scores = useMemo(() => personasSorted.map((p) => ({ score: p.stance_score, weight: p.confidence })), [personasSorted]);
  const stdev = useMemo(() => (scores.length ? stdDev(scores) : 0), [scores]);

  return (
    <div className="min-h-screen bg-gray-50 text-gray-900">
      <header className="mx-auto max-w-6xl px-4 py-8">
        <h1 className="text-3xl font-semibold tracking-tight">Debiaser</h1>
        <p className="mt-1 text-sm text-gray-600">Paste a link or text. Personas analyse, the debiaser synthesises, and placement appears on a spectrum.</p>
      </header>

      <main className="mx-auto max-w-6xl px-4 pb-24">
        <section className="grid grid-cols-1 gap-4 rounded-2xl bg-white p-4 shadow">
          <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
            <div className="md:col-span-2">
              <label className="text-sm font-medium">Article URL</label>
              <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://example.com/article" className="mt-1 w-full rounded-xl border border-gray-200 p-3 outline-none focus:ring" />
            </div>
            <div className="flex items-end">
              <button onClick={onAnalyse} disabled={loading} className="w-full rounded-xl bg-black px-4 py-3 text-white hover:opacity-90 disabled:opacity-50">{loading ? "Analysing..." : "Analyse"}</button>
            </div>
          </div>

          <div>
            <label className="text-sm font-medium">Or paste article text</label>
            <textarea value={text} onChange={(e) => setText(e.target.value)} placeholder="Paste raw article text here if the URL is paywalled." className="mt-1 h-36 w-full rounded-xl border border-gray-200 p-3 outline-none focus:ring" />
          </div>
        </section>

        {/* Spectrum */}
        <section className="mt-6 rounded-2xl bg-white p-4 shadow">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-lg font-semibold">Political spectrum</h2>
              <p className="text-sm text-gray-600">Single-axis placement from Liberty (−3) to Order (+3), inferred from personas.</p>
            </div>
            <label className="flex items-center gap-2 text-xs text-gray-600">
              <input type="checkbox" checked={show2D} onChange={(e) => setShow2D(e.target.checked)} /> Show Economic vs Social axes
            </label>
          </div>
          <SpectrumBar value={spectrum} />

          {/* Disagreement meter with cluster sizes */}
          {result && (
            <div className="mt-3 rounded-xl border border-gray-200 p-3 text-sm text-gray-800">
              <div className="flex items-center gap-3">
                <span className="font-medium">Disagreement meter:</span>
                <Meter stdev={stdev} />
                <span className="text-xs text-gray-600">Std dev {stdev.toFixed(2)}</span>
              </div>
              <div className="mt-1 text-xs text-gray-600">{clusters.length} cluster{clusters.length !== 1 ? "s" : ""} detected: {clusters.map((c, i) => `${c.length} persona${c.length !== 1 ? "s" : ""}`).join(", ")}</div>
            </div>
          )}
        </section>

        {/* 2D Axes */}
        {show2D && result && (
          <section className="mt-6 rounded-2xl bg-white p-4 shadow">
            <div className="flex items-center justify-between">
              <h3 className="text-md font-semibold">Economic vs Social axes</h3>
              <AxesLegend />
            </div>
            <AxisGrid personas={result.personas} />
          </section>
        )}

        {/* Results */}
        {error && <div className="mt-6 rounded-xl border border-red-200 bg-red-50 p-3 text-sm text-red-800">{error}</div>}

        {result && (
          <section className="mt-6 grid gap-6">
            <article className="rounded-2xl bg-white p-4 shadow">
              <h2 className="text-xl font-semibold">{result.title}</h2>
              {result.source_url && (
                <a className="text-sm text-blue-600" href={result.source_url} target="_blank" rel="noreferrer">{result.source_url}</a>
              )}
            </article>

            {/* Persona clusters */}
            {clusters.map((cluster, idx) => (
              <ClusterBlock key={idx} personas={cluster} index={idx} />
            ))}

            <section className="rounded-2xl bg-white p-4 shadow">
              <h3 className="text-lg font-semibold">Truth-seeking summary</h3>
              <p className="mt-2 text-gray-800">{result.debiaser.truth_seeking_summary}</p>
              <div className="mt-4 grid grid-cols-1 gap-3 md:grid-cols-3">
                <ListCard title="Consensus" items={result.debiaser.consensus_points} />
                <ListCard title="Disagreements" items={result.debiaser.disagreements} />
                <ListCard title="Likely bias drivers" items={result.debiaser.likely_bias_drivers} />
              </div>
            </section>
          </section>
        )}
      </main>
    </div>
  );
}

function PersonaCard({ p }: { p: PersonaOutput }) {
  return (
    <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} className="rounded-2xl border border-gray-100 bg-white p-4 shadow">
      <div className="flex items-center justify-between">
        <h4 className="text-base font-semibold">{p.title}</h4>
        <span className="rounded-full bg-gray-100 px-3 py-1 text-xs text-gray-700">Score {p.stance_score.toFixed(1)} · Conf {Math.round(p.confidence * 100)}%</span>
      </div>
      <p className="mt-2 text-sm text-gray-800">{p.summary}</p>
      <div className="mt-3">
        <h5 className="text-xs font-semibold uppercase tracking-wide text-gray-500">Key claims</h5>
        <ul className="mt-1 list-disc space-y-1 pl-5 text-sm text-gray-800">
          {p.key_claims.map((c, i) => <li key={i}>{c}</li>)}
        </ul>
      </div>
      <div className="mt-3">
        <h5 className="text-xs font-semibold uppercase tracking-wide text-gray-500">Fact checks</h5>
        <ul className="mt-1 space-y-2 text-sm text-gray-800">
          {p.fact_checks.map((fc, i) => (
            <li key={i} className="rounded-lg bg-gray-50 p-2">
              <div className="text-[13px] font-medium">{fc.claim}</div>
              <div className="text-[12px] text-gray-600">{fc.assessment} · {fc.rationale}</div>
            </li>
          ))}
        </ul>
      </div>
      {p.caveats.length > 0 && (
        <div className="mt-3">
          <h5 className="text-xs font-semibold uppercase tracking-wide text-gray-500">Caveats</h5>
          <ul className="mt-1 list-disc space-y-1 pl-5 text-sm text-gray-800">
            {p.caveats.map((c, i) => <li key={i}>{c}</li>)}
          </ul>
        </div>
      )}
    </motion.div>
  );
}

function ListCard({ title, items }: { title: string; items: string[] }) {
  return (
    <div className="rounded-2xl border border-gray-100 bg-white p-3 shadow">
      <h5 className="text-xs font-semibold uppercase tracking-wide text-gray-500">{title}</h5>
      <ul className="mt-1 list-disc space-y-1 pl-5 text-sm text-gray-800">
        {items.map((c, i) => <li key={i}>{c}</li>)}
      </ul>
    </div>
  );
}

function SpectrumBar({ value }: { value: number }) {
  const normalised = useMemo(() => Math.max(-3, Math.min(3, value)), [value]);
  const pct = ((normalised + 3) / 6) * 100; // 0..100
  const labels = ["-3", "-2", "-1", "0", "+1", "+2", "+3"];

  return (
    <div className="mt-3 rounded-2xl border border-gray-200 p-4">
      <div className="relative h-4 w-full rounded-full bg-gradient-to-r from-sky-200 via-gray-200 to-rose-200">
        <motion.div className="absolute left-0 top-0 h-4 rounded-full bg-gray-800" initial={{ width: 0 }} animate={{ width: `${pct}%` }} transition={{ type: "spring", stiffness: 120, damping: 20 }} />
        <motion.div className="absolute top-1/2 h-4 w-4 -translate-y-1/2 rounded-full border-2 border-white bg-black shadow" initial={{ left: 0 }} animate={{ left: `calc(${pct}% - 8px)` }} transition={{ type: "spring", stiffness: 120, damping: 20 }} />
      </div>
      <div className="mt-2 flex justify-between text-[11px] text-gray-600">{labels.map((l) => <span key={l}>{l}</span>)}</div>
      <div className="mt-1 text-xs text-gray-700">Value {normalised.toFixed(2)} on a −3 to +3 Liberty–Order axis.</div>
    </div>
  );
}

function Meter({ stdev }: { stdev: number }) {
  const level = stdev < 0.5 ? "Low" : stdev < 1.2 ? "Medium" : "High";
  const pct = Math.min(100, Math.round((stdev / 2) * 100));
  const colour = level === "Low" ? "bg-emerald-500" : level === "Medium" ? "bg-amber-500" : "bg-rose-500";
  return (
    <div className="flex-1">
      <div className="h-2 w-full rounded-full bg-gray-100">
        <div className={`h-2 rounded-full ${colour}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function ClusterBlock({ personas, index }: { personas: PersonaOutput[]; index: number }) {
  const min = Math.min(...personas.map((p) => p.stance_score));
  const max = Math.max(...personas.map((p) => p.stance_score));
  const tight = max - min < 0.8;
  const from = index % 2 === 0 ? "from-indigo-50" : "from-fuchsia-50";
  const to = tight ? (index % 2 === 0 ? "to-emerald-50" : "to-amber-50") : (index % 2 === 0 ? "to-sky-50" : "to-rose-50");
  return (
    <section className={`rounded-2xl border border-gray-100 bg-gradient-to-br ${from} ${to} p-4 shadow`}>
      <div className="mb-2 text-xs text-gray-600">Cluster {index + 1} · {personas.length} persona{personas.length !== 1 ? "s" : ""} · span {(max - min).toFixed(2)}</div>
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {personas.map((p) => <PersonaCard key={p.id} p={p} />)}
      </div>
    </section>
  );
}

function AxesLegend() {
  return (
    <div className="flex items-center gap-4 text-xs text-gray-700">
      <div className="rounded border border-gray-200 px-2 py-1">Economic: −3 more intervention ←→ +3 more market</div>
      <div className="rounded border border-gray-200 px-2 py-1">Social: −3 more libertarian/individual ←→ +3 more authoritarian/order</div>
      <div className="flex items-center gap-2"><span className="h-3 w-3 rounded-full" style={{ background: colourForAxes(-3, -3) }} /> to <span className="h-3 w-3 rounded-full" style={{ background: colourForAxes(3, 3) }} /> colour scale</div>
    </div>
  );
}

function AxisGrid({ personas }: { personas: PersonaOutput[] }) {
  const toPct = (v: number) => ((Math.max(-3, Math.min(3, v)) + 3) / 6) * 100;
  return (
    <div className="relative mt-3 h-64 w-full rounded-xl border border-gray-200 bg-white">
      {[0, 25, 50, 75, 100].map((p) => <div key={`h${p}`} className="absolute left-0 right-0" style={{ top: `${p}%` }}><div className="h-px w-full bg-gray-100" /></div>)}
      {[0, 25, 50, 75, 100].map((p) => <div key={`v${p}`} className="absolute top-0 bottom-0" style={{ left: `${p}%` }}><div className="w-px h-full bg-gray-100" /></div>)}
      <div className="absolute inset-0">
        <div className="absolute left-1/2 top-0 -ml-px h-full w-px bg-gray-200" />
        <div className="absolute top-1/2 left-0 -mt-px h-px w-full bg-gray-200" />
      </div>
      <div className="absolute left-1/2 top-2 -translate-x-1/2 text-xs text-gray-600">Economic (−3 ←→ +3)</div>
      <div className="absolute top-1/2 left-2 -translate-y-1/2 -rotate-90 text-xs text-gray-600">Social (−3 ←→ +3)</div>
      {personas.filter((p) => p.axes).map((p) => {
        const econ = p.axes!.economic; const soc = p.axes!.social;
        const bg = colourForAxes(econ, soc);
        return (
          <div key={p.id} className="absolute" style={{ left: `calc(${toPct(econ)}% - 7px)`, top: `calc(${100 - toPct(soc)}% - 7px)` }}>
            <div className="h-3.5 w-3.5 rounded-full shadow ring-2 ring-white" style={{ background: bg }} title={`${p.title}: econ ${econ.toFixed(1)}, social ${soc.toFixed(1)}`} />
          </div>
        );
      })}
    </div>
  );
}
