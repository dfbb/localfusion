import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { toast } from 'sonner'
import { useModels } from './models-provider'
import { Label as LabelPrimitive, RadioGroup as RadioGroupPrimitive } from 'radix-ui'
import { api } from '@/lib/api'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { modelSchema, type ModelForm, type ModelRow } from '../data/schema'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  currentRow?: ModelRow | null
}

const defaultValues: ModelForm = {
  id: '',
  connector: 'chat',
  base_url: '',
  api_key: '',
  api_key_env: '',
  model: '',
  anthropic_version: '',
  extra: '',
}

const DEFAULT_MAX_INPUT_TOKENS = 1_000_000

/** Read default_max_tokens (auto-detected output cap) from an extra JSON string. */
function parseMaxTokens(extra: string | null | undefined): number | null {
  if (!extra) return null
  try {
    const v = (JSON.parse(extra) as Record<string, unknown>)?.default_max_tokens
    return typeof v === 'number' ? v : null
  } catch {
    return null
  }
}

/** Read max_input_tokens (user-editable context window) from extra, defaulting to 1M. */
function parseMaxInputTokens(extra: string | null | undefined): number {
  if (!extra) return DEFAULT_MAX_INPUT_TOKENS
  try {
    const v = (JSON.parse(extra) as Record<string, unknown>)?.max_input_tokens
    return typeof v === 'number' ? v : DEFAULT_MAX_INPUT_TOKENS
  } catch {
    return DEFAULT_MAX_INPUT_TOKENS
  }
}

/** Merge max_input_tokens into the extra JSON string, preserving all other keys. */
function withMaxInputTokens(extra: string | undefined, maxInputTokens: number): string {
  let obj: Record<string, unknown> = {}
  if (extra) {
    try {
      const parsed = JSON.parse(extra) as Record<string, unknown>
      if (parsed && typeof parsed === 'object') obj = parsed
    } catch {
      /* malformed — start fresh */
    }
  }
  obj.max_input_tokens = maxInputTokens
  return JSON.stringify(obj)
}

export function ModelsActionDialog({ open, onOpenChange, currentRow }: Props) {
  const isEdit = !!currentRow
  const qc = useQueryClient()
  const { runTestOne } = useModels()
  const [keyMode, setKeyMode] = useState<'direct' | 'env'>('direct')
  // Auto-detected max output tokens, persisted in extra.default_max_tokens; shown read-only.
  const maxTokens = parseMaxTokens(currentRow?.extra)
  // User-editable context window (extra.max_input_tokens), defaulting to 1M.
  const [maxInputTokens, setMaxInputTokens] = useState<number>(DEFAULT_MAX_INPUT_TOKENS)

  const {
    register,
    handleSubmit,
    watch,
    setValue,
    reset,
    formState: { errors },
  } = useForm<ModelForm>({
    resolver: zodResolver(modelSchema),
    defaultValues,
  })

  // Reset form when dialog opens/closes or currentRow changes
  useEffect(() => {
    if (open) {
      if (currentRow) {
        reset({
          id: currentRow.id,
          connector: currentRow.connector as ModelForm['connector'],
          base_url: currentRow.base_url,
          api_key: '',
          api_key_env: currentRow.api_key_env ?? '',
          model: currentRow.model,
          anthropic_version: currentRow.anthropic_version ?? '',
          extra: currentRow.extra ?? '',
        })
        setMaxInputTokens(parseMaxInputTokens(currentRow.extra))
        // 直填密钥的模型(api_key_enc 有值)即便 reset 清空 api_key 也应保持「直填」模式
        setKeyMode(currentRow.api_key_enc ? 'direct' : currentRow.api_key_env ? 'env' : 'direct')
      } else {
        reset(defaultValues)
        setMaxInputTokens(DEFAULT_MAX_INPUT_TOKENS)
        setKeyMode('direct')
      }
    }
  }, [open, currentRow, reset])

  const m = useMutation({
    mutationFn: (v: ModelForm) =>
      isEdit ? api.put(`/models/${v.id}`, v) : api.post('/models', v),
    onSuccess: (_data, v) => {
      qc.invalidateQueries({ queryKey: ['models'] })
      toast.success('已保存')
      onOpenChange(false)
      if (isEdit) {
        // Fire-and-forget: probe the saved model to auto-correct base_url/connector if needed
        runTestOne(v.id)
      }
    },
    onError: () => toast.error('保存失败'),
  })

  const connector = watch('connector')

  function onSubmit(v: ModelForm) {
    // Strip empty optional fields
    const payload: ModelForm = { ...v }
    if (!payload.api_key) delete payload.api_key
    if (!payload.api_key_env) delete payload.api_key_env
    if (!payload.anthropic_version) delete payload.anthropic_version
    // Merge the user-edited context window into extra (preserves probe-managed keys).
    payload.extra = withMaxInputTokens(payload.extra, maxInputTokens)
    m.mutate(payload)
  }

  return (
    <Dialog open={open} onOpenChange={(s) => { if (!m.isPending) { reset(); onOpenChange(s) } }}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{isEdit ? '编辑模型' : '新建模型'}</DialogTitle>
          <DialogDescription>
            {isEdit ? '修改模型配置。' : '填写新模型信息。'}
            完成后点击保存。
          </DialogDescription>
        </DialogHeader>

        <form id="model-form" onSubmit={handleSubmit(onSubmit)} className="space-y-3 py-2">
          {/* ID */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium" htmlFor="model-id">
              ID
            </LabelPrimitive.Root>
            <Input
              id="model-id"
              className="col-span-4"
              placeholder="my-gpt4"
              disabled={isEdit}
              {...register('id')}
            />
            {errors.id && (
              <p className="col-span-4 col-start-3 text-xs text-destructive">{errors.id.message}</p>
            )}
          </div>

          {/* Connector */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium">
              连接器
            </LabelPrimitive.Root>
            <div className="col-span-4">
              <Select
                value={watch('connector')}
                onValueChange={(v) => setValue('connector', v as ModelForm['connector'])}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="chat">chat</SelectItem>
                  <SelectItem value="anthropic">anthropic</SelectItem>
                  <SelectItem value="responses">responses</SelectItem>
                </SelectContent>
              </Select>
            </div>
            {errors.connector && (
              <p className="col-span-4 col-start-3 text-xs text-destructive">{errors.connector.message}</p>
            )}
          </div>

          {/* Base URL */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium" htmlFor="model-base-url">
              Base URL
            </LabelPrimitive.Root>
            <Input
              id="model-base-url"
              className="col-span-4"
              placeholder="https://api.openai.com/v1"
              {...register('base_url')}
            />
            {errors.base_url && (
              <p className="col-span-4 col-start-3 text-xs text-destructive">{errors.base_url.message}</p>
            )}
          </div>

          {/* Model */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium" htmlFor="model-name">
              模型名
            </LabelPrimitive.Root>
            <Input
              id="model-name"
              className="col-span-4"
              placeholder="gpt-4o"
              {...register('model')}
            />
            {errors.model && (
              <p className="col-span-4 col-start-3 text-xs text-destructive">{errors.model.message}</p>
            )}
          </div>

          {/* Anthropic Version (only for anthropic connector) */}
          {connector === 'anthropic' && (
            <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
              <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium" htmlFor="model-av">
                API Version
              </LabelPrimitive.Root>
              <Input
                id="model-av"
                className="col-span-4"
                placeholder="2023-06-01"
                {...register('anthropic_version')}
              />
            </div>
          )}

          {/* API Key section */}
          <div className="grid grid-cols-6 gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium pt-2">
              密钥
            </LabelPrimitive.Root>
            <div className="col-span-4 space-y-2">
              <RadioGroupPrimitive.Root
                value={keyMode}
                onValueChange={(v) => {
                  setKeyMode(v as 'direct' | 'env')
                  if (v === 'direct') setValue('api_key_env', '')
                  else setValue('api_key', '')
                }}
                className="flex gap-4"
              >
                <div className="flex items-center gap-1.5">
                  <RadioGroupPrimitive.Item
                    value="direct"
                    id="key-direct"
                    className="h-4 w-4 rounded-full border border-primary text-primary focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 data-[state=checked]:bg-primary"
                  >
                    <RadioGroupPrimitive.Indicator className="flex items-center justify-center">
                      <span className="h-2 w-2 rounded-full bg-background block" />
                    </RadioGroupPrimitive.Indicator>
                  </RadioGroupPrimitive.Item>
                  <LabelPrimitive.Root htmlFor="key-direct" className="text-sm cursor-pointer">
                    直填
                  </LabelPrimitive.Root>
                </div>
                <div className="flex items-center gap-1.5">
                  <RadioGroupPrimitive.Item
                    value="env"
                    id="key-env"
                    className="h-4 w-4 rounded-full border border-primary text-primary focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 data-[state=checked]:bg-primary"
                  >
                    <RadioGroupPrimitive.Indicator className="flex items-center justify-center">
                      <span className="h-2 w-2 rounded-full bg-background block" />
                    </RadioGroupPrimitive.Indicator>
                  </RadioGroupPrimitive.Item>
                  <LabelPrimitive.Root htmlFor="key-env" className="text-sm cursor-pointer">
                    环境变量
                  </LabelPrimitive.Root>
                </div>
              </RadioGroupPrimitive.Root>

              {keyMode === 'direct' ? (
                <Input
                  type="password"
                  placeholder={isEdit && currentRow?.api_key_enc ? '已设置（留空不变）' : 'sk-...'}
                  autoComplete="new-password"
                  {...register('api_key')}
                />
              ) : (
                <Input
                  placeholder="OPENAI_API_KEY"
                  {...register('api_key_env')}
                />
              )}
            </div>
          </div>

          {/* Max input tokens — user-editable context window (extra.max_input_tokens) */}
          <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
            <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium" htmlFor="model-max-input-tokens">
              Max Input Tokens
            </LabelPrimitive.Root>
            <Input
              id="model-max-input-tokens"
              className="col-span-4"
              type="number"
              min={1}
              value={maxInputTokens}
              onChange={(e) => setMaxInputTokens(Number(e.target.value) || 0)}
              placeholder="1000000"
            />
            <p className="col-span-4 col-start-3 text-xs text-muted-foreground">
              上下文窗口（输入上限），默认 1,000,000，仅作记录参考，不强制校验
            </p>
          </div>

          {/* Max output tokens — auto-detected by connectivity probing, read-only */}
          {isEdit && (
            <div className="grid grid-cols-6 items-center gap-x-4 gap-y-1">
              <LabelPrimitive.Root className="col-span-2 text-end text-sm font-medium" htmlFor="model-max-tokens">
                Max Output Tokens
              </LabelPrimitive.Root>
              <Input
                id="model-max-tokens"
                className="col-span-4"
                value={maxTokens != null ? maxTokens.toLocaleString() : '未检测'}
                readOnly
                disabled
                placeholder="未检测"
              />
              <p className="col-span-4 col-start-3 text-xs text-muted-foreground">
                输出上限，由连接测试自动检测，不可编辑
              </p>
            </div>
          )}

          {/* extra is carried through hidden — probe-managed (default_max_tokens) + max_input_tokens */}
          <input type="hidden" {...register('extra')} />
        </form>

        <DialogFooter>
          <Button
            type="submit"
            form="model-form"
            disabled={m.isPending}
          >
            {m.isPending ? '保存中…' : '保存'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
