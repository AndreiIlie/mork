import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { before, test } from 'node:test'
import { fileURLToPath } from 'node:url'

const standalone = [
  '// comments mentioning rounded-full or -mx-3 are ignored',
  '/* block comment with lg:-mt-2 */',
  'const ok = "mx-3 border px-edge";',
  'const a = "h-2 w-2 rounded-full";',
  'const b = "-mx-3 mt-3 divide-y";',
  'const c = "lg:-mt-2 hover:rounded";',
  'const d = "hover:rounded-[length:3px] -mx-[length:3px]";',
  'const e = `flex ${ok ? "-my-2" : ""} lg:rounded-sm`;',
].join('\n')

const sources = {
  'src/Board.tsx': standalone,
  'src/ui/Dot.tsx': standalone,
  'src/ClassProbe.tsx': `export const Upload = () => <input type="file" accept="image/*" className="-mx-3" />;
export const Marker = () => <div className="rounded-full" />;
export const Separator = () => <div className="before:content-['//'] -my-2" />;
const rounded = Math.round(2.5);
export const Total = () => <p title="Durations are rounded to whole minutes">{"Totals are rounded to the nearest dollar."} {rounded}</p>;
export const Text = () => <div className="before:content-['rounded-full']" />;
const re = /[.,;:!?'"”’]$/;
const prose = "Values are rounded to whole minutes.";
export const AfterRe = () => <div className={prose ? "rounded" : "-ml-2"} />;
export const caption = "The total is rounded to whole minutes.";`,
  'src/Wrapped.tsx': `export const Wrapped = () => (
  <div className="
    flex rounded-[13px]
    -mx-[17px]
  " />
);
export const Inline = () => <section><p>Totals</p><div className="rounded-[19px] -mx-[23px]" /></section>;`,
  'src/Copy.tsx': `export function PrecisionBadge() {
  return (
    // Keep the caption short.
    <span title="rounded">{"rounded"}</span>
  );
}
const HINT = "Displays a rounded-down percentage.";
export const Hint = () => <span title={HINT}>%</span>;`,
  'src/Steps.tsx': `export function Steps({ steps }: { steps: string[] }) {
  return (
    <ol>
      {steps.map((s) => {
        const tone = s === "now" ? "font-medium rounded-[13px] -mx-[17px]" : "text-gray-700";
        return <li key={s} className={tone}>{s}</li>;
      })}
    </ol>
  );
}
const ROW = "flex rounded";
export const List = () => <div className={ROW}>Rows</div>;`,
  'src/Spread.tsx': `function Caption() {
  const value = "-mx-2";
  return <div {...{ title: value }} />;
}
function Badge() {
  const value = "a rounded";
  const rowClassName = value;
  const props = { rowClassName, title: "-mx-3" };
  const alias = { ...props };
  return <Row {...alias} {...{ class: "hover:!rounded-sm!" }} />;
}`,
  'src/extra.css': `/* @apply rounded-full -mx-3; */
.card {
  content: "/*";
  @apply hover:rounded-[length:3px] -mx-3;
  @media (width > 600px) {
    @apply/* rounded-full */
      rounded-tl /* -mx-5
        rounded-sm */ -mb-edge!;
  }
  @variant rounded-full { content: "@apply -mx-3"; }
}`,
}
let findings
before(() => {
  const cwd = mkdtempSync(join(tmpdir(), 'web-classes-'))
  try {
    for (const [file, text] of Object.entries(sources)) {
      mkdirSync(join(cwd, dirname(file)), { recursive: true })
      writeFileSync(join(cwd, file), text)
    }
    writeFileSync(join(cwd, 'tsconfig.json'), '{"compilerOptions":{"jsx":"preserve"},"include":["src"]}')
    const script = fileURLToPath(new URL('./check-classes.mjs', import.meta.url))
    findings = JSON.parse(execFileSync(process.execPath, [script], { cwd, encoding: 'utf8' }))
  } finally {
    rmSync(cwd, { recursive: true, force: true })
  }
})

function expect(file, expected) {
  const actual = findings.filter(f => f.file === file)
  assert.deepEqual(actual.map(f => [f.line, f.summary.split('`')[1]]), expected)
  for (const finding of actual) {
    const token = finding.summary.split('`')[1]
    assert.equal(finding.proof, `web/${file}:${finding.line}: ${token}\n  ${sources[file].split('\n')[finding.line - 1].trim()}`)
  }
}

test('standalone classes, template spans, arbitrary values and the ui exemption', () => {
  expect('src/Board.tsx', [[4, 'rounded-full'], [5, '-mx-3'], [6, 'lg:-mt-2'], [6, 'hover:rounded'],
    [7, 'hover:rounded-[length:3px]'], [7, '-mx-[length:3px]'], [8, '-my-2'], [8, 'lg:rounded-sm']])
  expect('src/ui/Dot.tsx', [])
})
test('comment markers, regexes, prose and non-class attributes', () => {
  expect('src/ClassProbe.tsx', [[1, '-mx-3'], [2, 'rounded-full'], [3, '-my-2'], [9, 'rounded'], [9, '-ml-2']])
})
test('multiline JSX classes and tags after closing tags', () => {
  expect('src/Wrapped.tsx', [[3, 'rounded-[13px]'], [4, '-mx-[17px]'], [7, 'rounded-[19px]'], [7, '-mx-[23px]']])
})
test('JSX callbacks, class constants and captions', () => {
  expect('src/Copy.tsx', [])
  expect('src/Steps.tsx', [[5, 'rounded-[13px]'], [5, '-mx-[17px]'], [11, 'rounded']])
})
test('spread properties, shorthand aliases and same-named locals', () => {
  expect('src/Spread.tsx', [[6, 'rounded'], [10, 'hover:!rounded-sm!']])
})
test('CSS @apply, multiline comments and other directives', () => {
  expect('src/extra.css', [[4, 'hover:rounded-[length:3px]'], [4, '-mx-3'], [7, 'rounded-tl'], [8, '-mb-edge!']])
})
