import type { ReactNode } from "react";
import { useReducedMotion, useSeen, useTicker } from "../hooks";
import { rulesTrait } from "../rulesSnippet";

const KEYWORDS = new Set(["pub", "trait", "const", "type", "fn", "self", "Self", "impl", "mut", "where"]);
const FUNCTIONS = new Set(["classify", "settle", "board"]);

/** Light Rust highlighting: keywords, the three functions, comments. */
function highlight(line: string, key: number): ReactNode {
  const out: ReactNode[] = [];
  const comment = line.indexOf("//");
  const code = comment >= 0 ? line.slice(0, comment) : line;
  const re = /[A-Za-z_][A-Za-z0-9_]*|[^A-Za-z_]+/g;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(code))) {
    const w = m[0];
    if (KEYWORDS.has(w)) out.push(<span key={i++} className="tok-kw">{w}</span>);
    else if (FUNCTIONS.has(w)) out.push(<span key={i++} className="tok-fn">{w}</span>);
    else if (w === "Rules") out.push(<span key={i++} className="tok-ty">{w}</span>);
    else out.push(w);
  }
  if (comment >= 0) out.push(<span key={i++} className="tok-com">{line.slice(comment)}</span>);
  return (
    <span key={key} className="line">
      {out}
    </span>
  );
}

const COMMAND = "zecraft new dice";
const OUTPUT = [
  "Created dice in games/dice:",
  "  games/dice/rules/src/lib.rs",
  "  games/dice/rules/tests/dice.rs",
  "  games/dice/server/src/bin/zecraft-dice-engine.rs",
  "  games/dice/server/src/bin/zecraft-dice-keyholder.rs",
  "  games/dice/server/src/bin/zecraft-dice-verify.rs",
];
const TEST_COMMAND = "cargo test --release -p zecraft-dice";
// Ticks of 100 ms: type the command, print the files, type the test command, pass.
const T_OUTPUT = COMMAND.length + 4;
const T_TEST = T_OUTPUT + OUTPUT.length * 2 + 4;
const T_PASSED = T_TEST + TEST_COMMAND.length + 6;

function Terminal() {
  const reduced = useReducedMotion();
  const [ref, seen] = useSeen<HTMLPreElement>();
  const animate = !reduced;
  const t0 = useTicker(100, animate && seen);
  const t = animate ? t0 : T_PASSED;
  const done = t >= T_PASSED;
  const typed = COMMAND.slice(0, Math.max(0, Math.min(COMMAND.length, t)));
  const lines = t < T_OUTPUT ? 0 : Math.min(OUTPUT.length, Math.floor((t - T_OUTPUT) / 2) + 1);
  const testTyped = t < T_TEST ? null : TEST_COMMAND.slice(0, t - T_TEST);
  const caretOnFirst = t < T_OUTPUT;
  return (
    <pre ref={ref} className="term-body" aria-label={done ? undefined : "Terminal: zecraft new dice, then cargo test"}>
      {/* While typing, screen readers get the finished transcript via the hidden copy below. */}
      <span aria-hidden={!done}>
        <span className="line">
          <span className="tok-prompt">$</span> <span className="term-cmd">{typed}</span>
          {caretOnFirst && <span className="caret">▌</span>}
        </span>
        {OUTPUT.slice(0, lines).map((l) => (
          <span key={l} className="line">
            {l}
          </span>
        ))}
        {testTyped !== null && (
          <span className="line">
            <span className="tok-prompt">$</span> <span className="term-cmd">{testTyped}</span>
            {!done && <span className="caret">▌</span>}
          </span>
        )}
        {done && <span className="line term-pass">6 tests passed</span>}
      </span>
      {!done && (
        <span className="sr-only">
          {["$ " + COMMAND, ...OUTPUT, "$ " + TEST_COMMAND, "6 tests passed"].join("\n")}
        </span>
      )}
    </pre>
  );
}

export function ThreeFunctions() {
  return (
    <section className="wrap section" aria-labelledby="rules-h">
      <div className="eyebrow">the Rules trait</div>
      <h2 id="rules-h" className="h2">
        Three functions. That's your game.
      </h2>
      <div className="code-row">
        <figure className="code-panel">
          <figcaption className="code-head">
            <span>crates/zecraft-core/src/rules.rs</span>
            <span className="code-lang">rust</span>
          </figcaption>
          <pre className="code-body" tabIndex={0} aria-label="The Rules trait, in Rust">
            <code>{rulesTrait.split("\n").map(highlight)}</code>
          </pre>
        </figure>
        <figure className="code-panel">
          <figcaption className="code-head term-head">
            <span className="term-dot" aria-hidden="true" />
            <span className="term-dot" aria-hidden="true" />
            <span className="term-dot" aria-hidden="true" />
            <span className="term-title">terminal</span>
          </figcaption>
          <Terminal />
        </figure>
      </div>
      <p className="section-caption">Zecraft runs the rounds, the draw, the vault and the verification.</p>
    </section>
  );
}
