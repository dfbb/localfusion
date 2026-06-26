import React, { useState } from 'react'
import { type ModelRow } from '../data/schema'

type ModelsDialogType = 'add' | 'edit' | 'delete'

type ModelsContextType = {
  open: ModelsDialogType | null
  setOpen: (str: ModelsDialogType | null) => void
  currentRow: ModelRow | null
  setCurrentRow: React.Dispatch<React.SetStateAction<ModelRow | null>>
}

const ModelsContext = React.createContext<ModelsContextType | null>(null)

export function ModelsProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState<ModelsDialogType | null>(null)
  const [currentRow, setCurrentRow] = useState<ModelRow | null>(null)

  return (
    <ModelsContext value={{ open, setOpen, currentRow, setCurrentRow }}>
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
