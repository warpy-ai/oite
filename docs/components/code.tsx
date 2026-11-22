"use client";

import "@/lib/prism-languages";
import { CodeBlock } from "@/components/ui/code-block";

const ALIASES: Record<string, string> = { sh: "bash", shell: "bash", js: "javascript", ts: "typescript" };

/** Fenced code block, rendered with Rare UI's CodeBlock. */
export function Code({ code, language }: { code: string; language?: string }) {
  const lang = language ? (ALIASES[language] ?? language) : "text";
  return (
    <CodeBlock
      code={code}
      language={lang}
      showHeader={Boolean(language)}
      showLineNumbers={false}
      className="not-prose my-6 rounded-xl"
    />
  );
}
