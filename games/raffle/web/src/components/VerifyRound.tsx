import { useEffect, useRef, useState } from "react";
import type { Check, Report, WorkerMessage } from "../verify.worker";

/** The five checks, in the order the verifier reports them. */
const CHECKS = ["Beacon", "Entries", "Timing", "Settlement", "Payout"];
const RUNNING_CHECKS = "Running the five checks";

type Phase = "idle" | "running" | "done" | "error";

export interface VerifyRequest {
  round: number;
  /** Bumped to start a run. 0 = just show the panel. */
  nonce: number;
}

export function VerifyRound({ request, reduced }: { request: VerifyRequest; reduced: boolean }) {
  const [round, setRound] = useState(request.round);
  const [phase, setPhase] = useState<Phase>("idle");
  const [step, setStep] = useState("");
  const [report, setReport] = useState<Report | null>(null);
  const [millis, setMillis] = useState(0);
  const [error, setError] = useState("");
  const [shown, setShown] = useState(0);
  const worker = useRef<Worker | null>(null);

  function run(n: number) {
    worker.current?.terminate();
    setRound(n);
    setReport(null);
    setError("");
    setShown(0);
    setStep("Starting");
    setPhase("running");
    const w = new Worker(new URL("../verify.worker.ts", import.meta.url), { type: "module" });
    worker.current = w;
    w.onmessage = (event: MessageEvent<WorkerMessage>) => {
      const m = event.data;
      if (m.kind === "progress") setStep(m.step);
      if (m.kind === "done") {
        setReport(m.report);
        setMillis(m.millis);
        setPhase("done");
        w.terminate();
      }
      if (m.kind === "error") {
        setError(m.message);
        setPhase("error");
        w.terminate();
      }
    };
    w.postMessage({ round: n });
  }

  // A request from elsewhere on the page (proof strip, past rounds).
  useEffect(() => {
    if (request.nonce > 0) run(request.round);
    else setRound(request.round);
  }, [request.nonce, request.round]);

  useEffect(() => () => worker.current?.terminate(), []);

  // Reveal the results one check at a time.
  useEffect(() => {
    if (phase !== "done" || !report) return;
    if (reduced) {
      setShown(report.checks.length);
      return;
    }
    let i = 0;
    const id = setInterval(() => {
      i += 1;
      setShown(i);
      if (i >= report.checks.length) clearInterval(id);
    }, 140);
    return () => clearInterval(id);
  }, [phase, report, reduced]);

  const revealed = phase === "done" && report !== null && shown >= report.checks.length;
  const failed = report ? report.checks.filter((c) => !c.pass).length : 0;
  const checking = phase === "running" && step.startsWith(RUNNING_CHECKS);

  return (
    <div id="verify" className="panel verify" data-component="VerifyRound" data-round={round} data-phase={revealed ? (report?.passed ? "passed" : "failed") : phase}>
      <div className="verify-head">
        <div>
          <div className="label">verify</div>
          <h4>Verify round {round}</h4>
          <p className="muted small">Runs the five checks in your browser against testnet data.</p>
        </div>
        <button type="button" className={phase === "idle" ? "btn btn-primary" : "btn btn-ghost"} onClick={() => run(round)} disabled={phase === "running"}>
          {phase === "running" ? "Verifying..." : phase === "idle" ? `Verify round ${round}` : "Run again"}
        </button>
      </div>
      <div aria-live="polite">
        {phase === "idle" && <div className="verify-box">5 checks: Beacon, Entries, Timing, Settlement, Payout. Nothing to install.</div>}
        {(phase === "running" || (phase === "done" && !revealed)) && (
          <div className="verify-box verify-box-run mono">{phase === "running" ? `${step}...` : "Checking results..."}</div>
        )}
        {revealed && report && (
          <div className={`verify-box ${report.passed ? "verify-box-pass" : "verify-box-fail"}`}>
            <span className={report.passed ? "tag tag-pass" : "tag tag-fail"}>{report.passed ? "pass" : "fail"}</span>
            <span className="verify-verdict">
              {report.passed ? `Verified in your browser in ${(millis / 1000).toFixed(1)} s` : `${failed} of ${report.checks.length} checks failed. Don't trust this result.`}
            </span>
          </div>
        )}
        {phase === "error" && (
          <div className="verify-box verify-box-fail" role="alert">
            <span className="tag tag-fail">error</span>
            <span className="verify-verdict">Verification could not run: {error}</span>
          </div>
        )}
      </div>
      <ul className="checks">
        {CHECKS.map((name, i) => {
          const c: Check | undefined = report?.checks[i];
          const show = phase === "done" && c && i < shown;
          const state = show ? (c.pass ? "pass" : "fail") : phase === "running" && checking ? "run" : phase === "done" ? "run" : "pend";
          const text = show ? c.summary : state === "run" ? "checking..." : "waiting";
          const badge = show ? c.status.replace("_", " ") : state === "run" ? "running" : "";
          return (
            <li key={name} className={`check check-${state}`} data-check={name}>
              <span className="check-name" title={c?.name}>
                <span className="sq" aria-hidden="true" />
                {name}
              </span>
              <span className="check-text">{text}</span>
              <span className="check-badge">{badge}</span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
