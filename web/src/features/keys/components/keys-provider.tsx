import React, { useState } from 'react'
import { type KeyRow, type KeyCreateResult } from '../data/schema'

type KeysDialogType = 'create' | 'result' | 'acl' | 'edit-label' | 'delete'

type KeysContextType = {
  open: KeysDialogType | null
  setOpen: (str: KeysDialogType | null) => void
  currentRow: KeyRow | null
  setCurrentRow: React.Dispatch<React.SetStateAction<KeyRow | null>>
  createResult: KeyCreateResult | null
  setCreateResult: React.Dispatch<React.SetStateAction<KeyCreateResult | null>>
}

const KeysContext = React.createContext<KeysContextType | null>(null)

export function KeysProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState<KeysDialogType | null>(null)
  const [currentRow, setCurrentRow] = useState<KeyRow | null>(null)
  const [createResult, setCreateResult] = useState<KeyCreateResult | null>(null)

  return (
    <KeysContext value={{ open, setOpen, currentRow, setCurrentRow, createResult, setCreateResult }}>
      {children}
    </KeysContext>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useKeys() {
  const ctx = React.useContext(KeysContext)
  if (!ctx) throw new Error('useKeys must be used within <KeysProvider>')
  return ctx
}
