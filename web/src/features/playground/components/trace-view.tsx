import { useTranslation } from 'react-i18next'
import { Badge } from '@/components/ui/badge'
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'

type TraceViewProps = {
  strategy: string
  detail: Record<string, unknown>
}

function toStr(v: unknown): string {
  if (typeof v === 'string') return v
  return JSON.stringify(v, null, 2)
}

function truthy(v: unknown): boolean {
  return !!v
}

function StatusBadge({ status }: { status?: string }) {
  if (!status) return null
  const isOk = status === 'ok' || status === 'done' || status === 'success'
  return (
    <Badge variant={isOk ? 'default' : 'destructive'}>{status}</Badge>
  )
}

function MemberAnswers({ answers }: { answers: unknown[] }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">{t('playground.memberAnswers', { count: answers.length })}</p>
      <div className="space-y-2">
        {answers.map((a, i) => {
          const ans = a as Record<string, unknown>
          const usage = ans.usage as Record<string, unknown> | undefined
          const tokens =
            usage != null
              ? Number(usage.input_tokens ?? 0) + Number(usage.output_tokens ?? 0)
              : undefined
          return (
            <div key={i} className="rounded border p-3 text-sm space-y-1">
              <div className="flex items-center gap-2">
                <span className="text-muted-foreground">#{i + 1}</span>
                {truthy(ans.model_id) && <span className="font-mono text-xs">{String(ans.model_id)}</span>}
                {usage?.status != null && <StatusBadge status={String(usage.status)} />}
                {tokens != null && <span className="text-muted-foreground text-xs">{tokens} tok</span>}
              </div>
              <p className="whitespace-pre-wrap">{toStr(ans.text)}</p>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function JudgePanel({ judge }: { judge: Record<string, unknown> }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">{t('playground.judgeLabel')}</p>
      {truthy(judge.input) && (
        <div className="rounded border p-3 text-xs font-mono whitespace-pre-wrap max-h-40 overflow-auto">
          <span className="text-muted-foreground block mb-1">{t('playground.judgeInput')}</span>
          {toStr(judge.input)}
        </div>
      )}
      {truthy(judge.output) && (
        <div className="rounded border p-3 text-xs font-mono whitespace-pre-wrap max-h-40 overflow-auto">
          <span className="text-muted-foreground block mb-1">{t('playground.judgeOutput')}</span>
          {toStr(judge.output)}
        </div>
      )}
    </div>
  )
}

function AttemptsTimeline({ attempts }: { attempts: unknown[] }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">{t('playground.attemptsTimeline', { count: attempts.length })}</p>
      <div className="space-y-2">
        {attempts.map((a, i) => {
          const attempt = a as Record<string, unknown>
          const ok = attempt.ok === true
          return (
            <div key={i} className="flex items-start gap-3 rounded border p-3 text-sm">
              <span className="text-muted-foreground shrink-0">#{i + 1}</span>
              <div className="min-w-0 flex-1 space-y-1">
                {truthy(attempt.model_id) && (
                  <span className="font-mono text-xs">{String(attempt.model_id)}</span>
                )}
                {attempt.ok != null && <StatusBadge status={ok ? 'ok' : 'failed'} />}
                {truthy(attempt.error) && (
                  <p className="text-destructive text-xs">{String(attempt.error)}</p>
                )}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function CandidatesTable({ candidates }: { candidates: unknown[] }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">{t('playground.candidatesTable', { count: candidates.length })}</p>
      <div className="space-y-2">
        {candidates.map((c, i) => {
          const cand = c as Record<string, unknown>
          const metric = (cand.metric ?? {}) as Record<string, unknown>
          const throughput = metric.avg_throughput
          const cost = metric.est_cost
          return (
            <div key={i} className="flex flex-wrap gap-4 rounded border p-3 text-sm">
              {truthy(cand.model_id) && <span className="font-mono text-xs">{String(cand.model_id)}</span>}
              {throughput != null && (
                <span className="text-muted-foreground text-xs">{t('playground.throughput', { value: Number(throughput).toFixed(1) })}</span>
              )}
              {cost != null && (
                <span className="text-muted-foreground text-xs">{t('playground.estCost', { value: Number(cost).toFixed(6) })}</span>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

function TurnsTimeline({ turns }: { turns: unknown[] }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">{t('playground.turnsTimeline', { count: turns.length })}</p>
      <div className="space-y-2">
        {turns.map((t, i) => {
          const turn = t as Record<string, unknown>
          const isTool = truthy(turn.tool)
          return (
            <div key={i} className="rounded border p-3 text-sm">
              <div className="flex items-center gap-2 mb-1">
                <span className="text-muted-foreground text-xs">Turn {i + 1}</span>
                {isTool ? (
                  <>
                    <Badge variant="outline">tool</Badge>
                    <span className="font-mono text-xs">{String(turn.tool)}</span>
                    {truthy(turn.route) && (
                      <span className="text-muted-foreground text-xs">→ {String(turn.route)}</span>
                    )}
                  </>
                ) : (
                  <Badge variant="outline">main</Badge>
                )}
              </div>
              {truthy(turn.main_output) && (
                <p className="text-xs whitespace-pre-wrap">{toStr(turn.main_output)}</p>
              )}
              {truthy(turn.result) && (
                <p className="text-xs whitespace-pre-wrap text-muted-foreground">{toStr(turn.result)}</p>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

export function TraceView({ strategy, detail }: TraceViewProps) {
  const { t } = useTranslation()
  const status = typeof detail.status === 'string' ? detail.status : undefined

  const isPanelStrategy = strategy === 'synthesize' || strategy === 'best_of_n' || strategy === 'best-of-n'
  const isMultimodal = strategy === 'multimodal'

  const knownKeys = new Set(['status', 'attempts', 'candidates', 'turns', 'member_answers', 'judge'])
  const hasExtras = !isPanelStrategy && !isMultimodal &&
    !Array.isArray(detail.attempts) && !Array.isArray(detail.candidates) &&
    Object.keys(detail).some(k => !knownKeys.has(k))

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-3">
          <CardTitle className="text-base">{t('playground.traceTitle')}</CardTitle>
          <Badge variant="outline">{strategy}</Badge>
          {status && <StatusBadge status={status} />}
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {isPanelStrategy && (
          <div className="grid gap-4 md:grid-cols-2">
            <div>
              {Array.isArray(detail.member_answers) && (
                <MemberAnswers answers={detail.member_answers} />
              )}
            </div>
            <div>
              {detail.judge != null && typeof detail.judge === 'object' && (
                <JudgePanel judge={detail.judge as Record<string, unknown>} />
              )}
            </div>
          </div>
        )}

        {isMultimodal && Array.isArray(detail.turns) && (
          <TurnsTimeline turns={detail.turns} />
        )}

        {!isPanelStrategy && !isMultimodal && (
          <div className="space-y-4">
            {Array.isArray(detail.attempts) && (
              <AttemptsTimeline attempts={detail.attempts} />
            )}
            {Array.isArray(detail.candidates) && (
              <>
                {Array.isArray(detail.attempts) && <Separator />}
                <CandidatesTable candidates={detail.candidates} />
              </>
            )}
          </div>
        )}

        {hasExtras && (
          <div className="rounded border p-3">
            <pre className="text-xs whitespace-pre-wrap max-h-60 overflow-auto">
              {JSON.stringify(detail, null, 2)}
            </pre>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
