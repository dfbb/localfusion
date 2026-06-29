// Asserts zh.json and en.json have identical flattened key sets.
// Exits 0 on parity, 1 on any missing/extra key. No deps (pure Node).
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const localesDir = join(here, '..', 'src', 'i18n', 'locales')

function flatKeys(obj, prefix = '') {
  const out = []
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object' && !Array.isArray(v)) out.push(...flatKeys(v, key))
    else out.push(key)
  }
  return out
}

function load(name) {
  return new Set(flatKeys(JSON.parse(readFileSync(join(localesDir, name), 'utf8'))))
}

const zh = load('zh.json')
const en = load('en.json')
const missingInEn = [...zh].filter((k) => !en.has(k)).sort()
const extraInEn = [...en].filter((k) => !zh.has(k)).sort()

if (missingInEn.length || extraInEn.length) {
  if (missingInEn.length) console.error(`Missing in en.json (${missingInEn.length}):\n  ${missingInEn.join('\n  ')}`)
  if (extraInEn.length) console.error(`Extra in en.json (${extraInEn.length}):\n  ${extraInEn.join('\n  ')}`)
  process.exit(1)
}
console.log(`i18n key parity OK (${zh.size} keys).`)
