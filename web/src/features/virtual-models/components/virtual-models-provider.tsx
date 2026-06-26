import React, { useState } from 'react'
import { type VirtualModelRow } from '../data/schema'

type VirtualModelsDialogType = 'add' | 'edit' | 'delete'

type VirtualModelsContextType = {
  open: VirtualModelsDialogType | null
  setOpen: (str: VirtualModelsDialogType | null) => void
  currentRow: VirtualModelRow | null
  setCurrentRow: React.Dispatch<React.SetStateAction<VirtualModelRow | null>>
}

const VirtualModelsContext = React.createContext<VirtualModelsContextType | null>(null)

export function VirtualModelsProvider({ children }: { children: React.ReactNode }) {
  const [open, setOpen] = useState<VirtualModelsDialogType | null>(null)
  const [currentRow, setCurrentRow] = useState<VirtualModelRow | null>(null)

  return (
    <VirtualModelsContext value={{ open, setOpen, currentRow, setCurrentRow }}>
      {children}
    </VirtualModelsContext>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useVirtualModels() {
  const ctx = React.useContext(VirtualModelsContext)
  if (!ctx) throw new Error('useVirtualModels must be used within <VirtualModelsProvider>')
  return ctx
}
