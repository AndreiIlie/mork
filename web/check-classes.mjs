import { readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { Input, parse } from 'postcss'
import tokenize from 'postcss/lib/tokenize'
import { SyntaxKind as K } from 'typescript/unstable/ast'
import { API } from 'typescript/unstable/sync'

const sides = 't|r|b|l|s|e|tl|tr|br|bl|ss|se|ee|es'
const size = 'none|xs|sm|md|lg|xl|[234]xl|full'
const arbitrary = String.raw`\[.*\]|\(.*\)`
const radius = new RegExp(`^rounded(?:-(?:(?:${sides})(?:-|$))?(?:${size}|${arbitrary})?)?$`)
const margin = new RegExp(`^-m[xytrbles]?-(?:[0-9].*|px|auto|full|gutter|edge|${arbitrary})?$`)
const prose = /(?:^|\s)(?:[A-Z]|(?:a|an|and|are|as|at|be|by|for|from|in|is|it|its|not|of|on|or|that|the|to|was)(?=\s|$))|[.,;?](?=\s|$)/

function utility(token) {
  // Colons in arbitrary variants and values belong to their brackets.
  let depth = 0
  let start = 0
  for (let i = 0; i < token.length; i++) {
    if (token[i] === '[' || token[i] === '(') depth++
    if (token[i] === ']' || token[i] === ')') depth = Math.max(0, depth - 1)
    if (token[i] === ':' && depth === 0) start = i + 1
  }
  const base = token.slice(start).replace(/^!+|!+$/g, '')
  if (radius.test(base)) return 'rounded-corner'
  if (margin.test(base)) return 'negative-margin'
}

function cssClasses(file, text, report) {
  parse(text, { from: file }).walkAtRules('apply', rule => {
    const params = rule.raws.params?.raw ?? rule.params
    const offset = 1 + rule.name.length + rule.raws.afterName.length
    const tokens = tokenize(new Input(params))
    let start = 0
    while (!tokens.endOfFile()) {
      const pos = tokens.position()
      const [kind] = tokens.nextToken()
      if (kind === 'space' || kind === 'comment') {
        if (pos > start) report(params.slice(start, pos), rule.positionBy({ index: offset + start }).line)
        start = tokens.position()
      }
    }
    if (start < params.length) report(params.slice(start), rule.positionBy({ index: offset + start }).line)
  })
}

const literals = new Set([K.StringLiteral, K.NoSubstitutionTemplateLiteral, K.TemplateHead, K.TemplateMiddle, K.TemplateTail])
const jsxRoots = new Set([K.JsxElement, K.JsxFragment, K.JsxSelfClosingElement])
function propUse(name) {
  return /^(?:class|className)$|Class(?:Name)?$/.test(name) ? 'class' : 'text'
}

function tsClasses(source, checker, report) {
  const seen = { class: new Set(), text: new Set(), spread: new Set() }
  function follow(symbol, use) {
    const decl = symbol?.valueDeclaration
    if (decl?.path === source.path) mark(decl.resolve()?.initializer, use)
  }

  // One initializer can supply both a caption and a class, or be reached through alias cycles.
  function mark(node, use) {
    if (!node || seen[use].has(node) || jsxRoots.has(node.kind) || node.kind === K.Block) return
    seen[use].add(node)
    if (node.kind === K.Identifier || node.kind === K.PropertyAccessExpression) {
      follow(checker.getSymbolAtLocation(node.name ?? node), use)
    } else if (use === 'spread' && node.kind === K.ObjectLiteralExpression) {
      for (const prop of node.properties) {
        const field = propUse(prop.name?.text ?? prop.name?.expression?.text ?? '')
        if (prop.kind === K.SpreadAssignment) mark(prop.expression, 'spread')
        else if (prop.kind === K.ShorthandPropertyAssignment) follow(checker.getShorthandAssignmentValueSymbol(prop), field)
        else mark(prop.initializer, field)
      }
    } else if (node.kind === K.ConditionalExpression) {
      mark(node.condition, 'text')
      mark(node.whenTrue, use)
      mark(node.whenFalse, use)
    } else if (node.kind === K.BinaryExpression && node.operatorToken.kind === K.AmpersandAmpersandToken) {
      mark(node.left, 'text')
      mark(node.right, use)
    } else {
      node.forEachChild(child => mark(child, use))
    }
  }

  const strings = []
  function visit(node) {
    if (node.kind === K.JsxAttribute) mark(node.initializer, propUse(node.name.getText(source)))
    if (node.kind === K.JsxSpreadAttribute) mark(node.expression, 'spread')
    if (node.kind === K.JsxExpression && node.parent.kind !== K.JsxAttribute) mark(node.expression, 'text')
    if (literals.has(node.kind)) strings.push(node)
    node.forEachChild(visit)
  }
  visit(source)
  for (const node of strings) {
    const inClass = seen.class.has(node)
    if (!inClass && seen.text.has(node)) continue
    const start = node.getStart(source) + 1
    const suffix = node.kind === K.TemplateHead || node.kind === K.TemplateMiddle ? 2 : 1
    const raw = source.text.slice(start, node.end - suffix)
    const ignoreBare = !inClass && prose.test(raw)
    for (const match of raw.matchAll(/\S+/g)) {
      if (ignoreBare && match[0] === 'rounded') continue
      report(match[0], source.getLineAndCharacterOfPosition(start + match.index).line + 1)
    }
  }
}

function check() {
  const files = readdirSync('src', { recursive: true, withFileTypes: true })
    .filter(entry => entry.isFile() && /\.(?:tsx?|css)$/.test(entry.name))
    .map(entry => join(entry.parentPath, entry.name).replaceAll('\\', '/'))
    .filter(file => !file.startsWith('src/ui/') && !/\/(?:\.git|target|node_modules)\//.test(file))
    .sort()
  const findings = []
  const api = new API()
  try {
    const snapshot = api.updateSnapshot({ openFiles: files.filter(f => !f.endsWith('.css')).map(f => resolve(f)) })
    for (const file of files) {
      const text = readFileSync(file, 'utf8')
      if (!/rounded|-m[xytrbles]?-/.test(text)) continue
      const lines = text.split(/\r?\n/)
      function report(token, line) {
        const reason = utility(token)
        if (!reason) return
        findings.push({
          file, line,
          summary: `forbidden class \`${token}\`: ${reason} utility outside web/src/ui/`,
          proof: `web/${file}:${line}: ${token}\n  ${lines[line - 1].trim()}`,
        })
      }
      if (file.endsWith('.css')) cssClasses(file, text, report)
      else {
        const path = resolve(file)
        const project = snapshot.getDefaultProjectForFile(path)
        tsClasses(project.program.getSourceFile(path), project.checker, report)
      }
    }
  } finally {
    api.close()
  }
  return findings
}

console.log(JSON.stringify(check()))
