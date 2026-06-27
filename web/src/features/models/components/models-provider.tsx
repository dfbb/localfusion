import React, { useState } from 'react'
import { toast } from 'sonner'
import { useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { type ModelRow } from '../data/schema'

type ModelsDialogType = 'add' | 'edit' | 'delete'

export type TestResult =
  | { ok: true; latency_ms: number; max_tokens?: number; base_url_fixed?: string; connector_fixed?: string }
  | { ok: false; error: string }

// Raw JSON item returned by the backend probe endpoints.
type ProbeResponseItem = {
  id: string
  ok: boolean
  latency_ms?: number
  error?: string
  max_tokens?: number
  base_url_fixed?: string
  connector_fixed?: string
}

function toTestResult(item: ProbeResponseItem): TestResult {
  return item.ok
    ? {
        ok: true,
        latency_ms: item.latency_ms ?? 0, // ok:true responses always include latency_ms; fallback to 0 guards against schema drift
        max_tokens: item.max_tokens,
        base_url_fixed: item.base_url_fixed,
        connector_fixed: item.connector_fixed,
      }
    : { ok: false, error: item.error ?? 'unknown error' }
}

// True if a successful probe auto-corrected connector/base_url (used for the toast count).
function wasFixed(item: ProbeResponseItem): boolean {
  return !!(item.base_url_fixed || item.connector_fixed)
}

type ModelsContextType = {
  open: ModelsDialogType | null
  setOpen: (str: ModelsDialogType | null) => void
  currentRow: ModelRow | null
  setCurrentRow: React.Dispatch<React.SetStateAction<ModelRow | null>>
  testing: boolean
  testResults: Map<string, TestResult>
  runTestAll: () => Promise<void>
  runTestOne: (id: string) => void
}

const ModelsContext = React.createContext<ModelsContextType | null>(null)

export function ModelsProvider({ children }: { children: React.ReactNode }) {
  const qc = useQueryClient()
  const [open, setOpen] = useState<ModelsDialogType | null>(null)
  const [currentRow, setCurrentRow] = useState<ModelRow | null>(null)
  const [testing, setTesting] = useState(false)
  const [testResults, setTestResults] = useState<Map<string, TestResult>>(new Map())

  async function runTestAll() {
    setTesting(true)
    setTestResults(new Map())
    try {
      const resp = await api.post<ProbeResponseItem[]>('/models/test-all')
      const map = new Map<string, TestResult>()
      let fixedCount = 0
      for (const item of resp.data) {
        if (item.ok && wasFixed(item)) fixedCount++
        map.set(item.id, toTestResult(item))
      }
      setTestResults(map)
      // A probe may persist max_tokens even when connector/base_url were already correct,
      // so always refetch to reflect detected values; only toast when config was corrected.
      await qc.refetchQueries({ queryKey: ['models'] })
      if (fixedCount > 0) {
        toast.success(`Auto-corrected config for ${fixedCount} model(s)`)
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Test failed'
      toast.error(msg)
    } finally {
      setTesting(false)
    }
  }

  async function runTestOne(id: string) {
    setTestResults(prev => {
      const next = new Map(prev)
      // Mark as in-progress by removing any stale result for this id
      next.delete(id)
      return next
    })
    try {
      const resp = await api.post<ProbeResponseItem>(`/models/${id}/test`)
      const item = resp.data
      setTestResults(prev => new Map(prev).set(id, toTestResult(item)))
      if (item.ok) {
        // Refetch so detected max_tokens shows immediately; toast only when config changed.
        await qc.refetchQueries({ queryKey: ['models'] })
        if (wasFixed(item)) toast.success('Auto-corrected config')
      }
    } catch {
      // silently ignore — the user didn't explicitly request this test
    }
  }

  return (
    <ModelsContext value={{ open, setOpen, currentRow, setCurrentRow, testing, testResults, runTestAll, runTestOne }}>
      {children}
    </ModelsContext>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useModels() {
  const ctx = React.useContext(ModelsContext)
  if (!ctx) throw new Error('useModels must be used within <ModelsProvider>')
  return ctx
}
