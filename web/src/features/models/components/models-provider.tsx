import React, { useState } from 'react'
import { toast } from 'sonner'
import { useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api'
import { type ModelRow } from '../data/schema'

type ModelsDialogType = 'add' | 'edit' | 'delete'

export type TestResult =
  | { ok: true; latency_ms: number; base_url_fixed?: string }
  | { ok: false; error: string }

type ModelsContextType = {
  open: ModelsDialogType | null
  setOpen: (str: ModelsDialogType | null) => void
  currentRow: ModelRow | null
  setCurrentRow: React.Dispatch<React.SetStateAction<ModelRow | null>>
  testing: boolean
  testResults: Map<string, TestResult>
  runTestAll: () => Promise<void>
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
      const resp = await api.post<
        Array<{ id: string; ok: boolean; latency_ms?: number; error?: string; base_url_fixed?: string }>
      >('/models/test-all')
      const map = new Map<string, TestResult>()
      let fixedCount = 0
      for (const item of resp.data) {
        if (item.ok) {
          if (item.base_url_fixed) fixedCount++
          map.set(item.id, {
            ok: true,
            latency_ms: item.latency_ms ?? 0, // ok:true responses always include latency_ms; fallback to 0 guards against schema drift
            base_url_fixed: item.base_url_fixed,
          })
        } else {
          map.set(item.id, { ok: false, error: item.error ?? 'unknown error' })
        }
      }
      setTestResults(map)
      if (fixedCount > 0) {
        // base_url was auto-corrected in the DB; refresh the table to show new values
        qc.invalidateQueries({ queryKey: ['models'] })
        toast.success(`Auto-corrected base_url for ${fixedCount} model(s)`)
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Test failed'
      toast.error(msg)
    } finally {
      setTesting(false)
    }
  }

  return (
    <ModelsContext value={{ open, setOpen, currentRow, setCurrentRow, testing, testResults, runTestAll }}>
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
