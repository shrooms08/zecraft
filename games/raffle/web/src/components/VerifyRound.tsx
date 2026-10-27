import { useRef, useState } from "react";
import type { Report, WorkerMessage } from "../verify.worker";

export function VerifyRound({ round }: { round: number }) {
  const [step, setStep] = useState<string | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [millis, setMillis] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const worker = useRef<Worker | null>(null);

  function run() {
    worker.current?.terminate();
    setReport(null);
    setError(null);
    setStep("Starting");
    const w = new Worker(new URL("../verify.worker.ts", import.meta.url), { type: "module" });
    worker.current = w;
    w.onmessage = (event: MessageEvent<WorkerMessage>) => {
      const m = event.data;
      if (m.kind === "progress") setStep(m.step);
      if (m.kind === "done") {
        setReport(m.report);
        setMillis(m.millis);
        setStep(null);
        w.terminate();
      }
      if (m.kind === "error") {
        setError(m.message);
        setStep(null);
        w.terminate();
      }
    };
    w.postMessage({ round });
  }

  return (
    <div className="verify" data-round={round}>
      <button type="button" className="button" onClick={run} disabled={step !== null}>
        {step !== null ? "Verifying…" : `Verify round ${round}`}
      </button>
      {step && <p className="progress" aria-live="polite">{step}…</p>}
      {error && <p className="error" role="alert">Verification could not run: {error}</p>}
      {report && <VerifyResult report={report} millis={millis} />}
    </div>
  );
}

function VerifyResult({ report, millis }: { report: Report; millis: number | null }) {
  return (
    <div className="verify-result">
      <p className={report.passed ? "ok" : "error"}>
        {report.passed ? "All five checks passed" : "Verification FAILED"}
        {millis !== null && <small> ({(millis / 1000).toFixed(1)} s, in your browser)</small>}
      </p>
      <ol className="checks">
        {report.checks.map((c) => (
          <li key={c.name} data-pass={c.pass}>
            <strong>{c.pass ? "PASS" : "FAIL"}</strong> {c.name}: {c.summary}
          </li>
        ))}
      </ol>
    </div>
  );
}
