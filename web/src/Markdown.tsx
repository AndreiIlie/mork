import { memo } from "react";
import ReactMarkdown, { type Components } from "react-markdown";

interface MdNode {
  type: string;
  value?: string;
  url?: string;
  children?: MdNode[];
}

const SKIP_NODES = new Set(["link", "linkReference", "definition", "code", "inlineCode", "html"]);
const URL_RE = /https?:\/\/[^\s<>]+/g;
const TRAILING_PUNCT = /[.,;:!?'"”’]$/;
const CLOSE_BRACKET: Record<string, string> = { ")": "(", "]": "[", "}": "{" };

function trimUrl(raw: string): string {
  let end = raw.length;
  while (end > 0) {
    const ch = raw[end - 1];
    if (TRAILING_PUNCT.test(ch)) {
      end -= 1;
      continue;
    }
    const open = CLOSE_BRACKET[ch];
    if (open !== undefined) {
      const slice = raw.slice(0, end);
      if (slice.split(ch).length > slice.split(open).length) {
        end -= 1;
        continue;
      }
    }
    break;
  }
  return raw.slice(0, end);
}

function linkText(value: string): MdNode[] | null {
  URL_RE.lastIndex = 0;
  const out: MdNode[] = [];
  let cursor = 0;
  let changed = false;
  let match: RegExpExecArray | null;
  while ((match = URL_RE.exec(value)) !== null) {
    const start = match.index;
    if (start > 0 && /[A-Za-z0-9_]/.test(value[start - 1])) continue;
    const url = trimUrl(match[0]);
    if (url === "http://" || url === "https://") continue;
    if (start > cursor) out.push({ type: "text", value: value.slice(cursor, start) });
    out.push({ type: "link", url, children: [{ type: "text", value: url }] });
    cursor = start + url.length;
    changed = true;
  }
  if (!changed) return null;
  if (cursor < value.length) out.push({ type: "text", value: value.slice(cursor) });
  return out;
}

function walkLinks(node: MdNode): void {
  if (SKIP_NODES.has(node.type) || !Array.isArray(node.children)) return;
  const next: MdNode[] = [];
  let replaced = false;
  for (const child of node.children) {
    if (child.type === "text" && typeof child.value === "string") {
      const parts = linkText(child.value);
      if (parts !== null) {
        next.push(...parts);
        replaced = true;
        continue;
      }
    } else {
      walkLinks(child);
    }
    next.push(child);
  }
  if (replaced) node.children = next;
}

// Remark step: links bare http(s) URLs in prose without adding a markdown dependency or matching other schemes.
function autolinkBareUrls() {
  return (tree: unknown) => {
    if (typeof tree === "object" && tree !== null && "type" in tree) {
      walkLinks(tree as MdNode);
    }
  };
}

const REMARK_PLUGINS = [autolinkBareUrls];

// Styled strictly with web/DESIGN.md tokens; raw HTML is escaped and images stay plain text (no <img> fetches).
const BASE_COMPONENTS: Components = {
  p: ({ children }) => <p className="mb-2 last:mb-0">{children}</p>,
  strong: ({ children }) => <strong className="font-semibold text-gray-1000">{children}</strong>,
  a: ({ href, children }) => (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="text-gray-1000 underline decoration-gray-500 underline-offset-2 hover:decoration-gray-1000"
    >
      {children}
    </a>
  ),
  img: ({ alt }) => <span className="text-label-12 text-gray-700">{alt ? `[image: ${alt}]` : "[image]"}</span>,
  ul: ({ children }) => <ul className="mb-2 list-disc space-y-1 pl-5 last:mb-0">{children}</ul>,
  ol: ({ start, children }) => (
    <ol start={start} className="mb-2 list-decimal space-y-1 pl-5 last:mb-0">
      {children}
    </ol>
  ),
  blockquote: ({ children }) => (
    <blockquote className="mb-2 border-l border-gray-400 pl-3 text-gray-900 last:mb-0">{children}</blockquote>
  ),
  h1: ({ children }) => <p className="mb-1.5 font-semibold text-gray-1000 last:mb-0">{children}</p>,
  h2: ({ children }) => <p className="mb-1.5 font-semibold text-gray-1000 last:mb-0">{children}</p>,
  h3: ({ children }) => <p className="mb-1.5 font-semibold text-gray-1000 last:mb-0">{children}</p>,
  h4: ({ children }) => <p className="mb-1.5 font-semibold text-gray-1000 last:mb-0">{children}</p>,
  hr: () => <hr className="my-3 border-gray-400" />,
};

function makeComponents(codeBg: string): Components {
  return {
    ...BASE_COMPONENTS,
    pre: ({ children }) => (
      <pre
        className={`mb-2 overflow-x-auto border border-gray-400 ${codeBg} p-3 font-mono text-label-13 text-gray-1000 last:mb-0`}
      >
        {children}
      </pre>
    ),
    code: ({ children }) => (
      <code className={`${codeBg} px-1 font-mono text-label-13 text-gray-1000 [pre_&]:bg-transparent [pre_&]:p-0`}>
        {children}
      </code>
    ),
  };
}

const COMPONENTS = makeComponents("bg-gray-100");
const ELEVATED_COMPONENTS = makeComponents("bg-background-100");

export const Markdown = memo(function Markdown({
  text,
  className = "text-gray-1000",
  elevated = false,
}: {
  text: string;
  className?: string;
  elevated?: boolean;
}) {
  return (
    <div className={`max-w-[74ch] break-words text-copy-14 ${className}`}>
      <ReactMarkdown remarkPlugins={REMARK_PLUGINS} components={elevated ? ELEVATED_COMPONENTS : COMPONENTS}>
        {text}
      </ReactMarkdown>
    </div>
  );
});
