import { z } from 'zod'

export const modelSchema = z.object({
  id: z.string().min(1),
  connector: z.enum(['chat', 'anthropic', 'responses']),
  base_url: z.string().url(),
  api_key: z.string().optional(),
  api_key_env: z.string().optional(),
  model: z.string().min(1),
  anthropic_version: z.string().optional(),
  extra: z.string().optional(),
})

export type ModelForm = z.infer<typeof modelSchema>

export type ModelRow = {
  id: string
  connector: string
  base_url: string
  api_key_enc: string | null
  api_key_env: string | null
  model: string
  anthropic_version: string | null
  extra: string | null
}
