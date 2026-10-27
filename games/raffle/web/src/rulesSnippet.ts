// The Rules trait, read from the kit's source at build time and shortened
// (doc comments and the defaulted `validate` dropped) for the page.
import source from "../../../../crates/zecraft-core/src/rules.rs?raw";

function extract(text: string): string {
  const start = text.indexOf("pub trait Rules");
  const end = text.indexOf("\n}\n", start);
  if (start < 0 || end < 0) return "pub trait Rules { … }";
  const body = text.slice(start, end + 2);
  const lines = body.split("\n").filter((l) => !l.trim().startsWith("///"));
  const out: string[] = [];
  let skipping = false;
  for (const line of lines) {
    if (line.includes("fn validate(")) skipping = true;
    if (!skipping) out.push(line);
    if (skipping && line.trim() === "}") skipping = false;
  }
  return out.join("\n").replace(/\n{3,}/g, "\n\n").trim();
}

export const rulesTrait = extract(source);
