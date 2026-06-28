import { z } from 'zod'
import type { TFunction } from 'i18next'

export function makeVirtualModelSchema(t: TFunction) {
  return z.object({
    name: z.string().min(1, t('virtualModels.nameRequired')),
    strategy: z.string().min(1, t('virtualModels.strategyRequired')),
    members: z.array(z.string()).min(1, t('virtualModels.membersRequired')),
    params: z.record(z.string(), z.unknown()).optional(),
  })
}

export type VirtualModelForm = z.infer<ReturnType<typeof makeVirtualModelSchema>>

export type VirtualModelRow = {
  name: string
  strategy: string
  members: string[]
  params: Record<string, unknown>
}

export type StrategyParamSchema = {
  type: string
  description?: string
  enum?: string[]
  default?: unknown
  minimum?: number
  maximum?: number
  'x-ref'?: string
}

export type StrategyRow = {
  name: string
  description?: string
  params_schema?: {
    type: string
    properties?: Record<string, StrategyParamSchema>
    required?: string[]
  }
}
