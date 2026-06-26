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
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">成员回答（{answers.length}）</p>
      <div className="space-y-2">
        {answers.map((a, i) => (
          <div key={i} className="rounded border p-3 text-sm">
            <span className="text-muted-foreground mr-2">#{i + 1}</span>
            {toStr(a)}
          </div>
        ))}
      </div>
    </div>
  )
}

function JudgePanel({ judge }: { judge: Record<string, unknown> }) {
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">裁判</p>
      {truthy(judge.input) && (
        <div className="rounded border p-3 text-xs font-mono whitespace-pre-wrap max-h-40 overflow-auto">
          <span className="text-muted-foreground block mb-1">输入</span>
          {toStr(judge.input)}
        </div>
      )}
      {truthy(judge.output) && (
        <div className="rounded border p-3 text-xs font-mono whitespace-pre-wrap max-h-40 overflow-auto">
          <span className="text-muted-foreground block mb-1">输出</span>
          {toStr(judge.output)}
        </div>
      )}
    </div>
  )
}

function AttemptsTimeline({ attempts }: { attempts: unknown[] }) {
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">尝试链（{attempts.length}）</p>
      <div className="space-y-2">
        {attempts.map((a, i) => {
          const attempt = a as Record<string, unknown>
          return (
            <div key={i} className="flex items-start gap-3 rounded border p-3 text-sm">
              <span className="text-muted-foreground shrink-0">#{i + 1}</span>
              <div className="min-w-0 flex-1 space-y-1">
                {truthy(attempt.model_id) && (
                  <span className="font-mono text-xs">{String(attempt.model_id)}</span>
                )}
                {truthy(attempt.status) && <StatusBadge status={String(attempt.status)} />}
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
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">候选对比（{candidates.length}）</p>
      <div className="space-y-2">
        {candidates.map((c, i) => {
          const cand = c as Record<string, unknown>
          return (
            <div key={i} className="flex flex-wrap gap-4 rounded border p-3 text-sm">
              {truthy(cand.model_id) && <span className="font-mono text-xs">{String(cand.model_id)}</span>}
              {cand.latency_secs != null && (
                <span className="text-muted-foreground text-xs">延迟 {Number(cand.latency_secs).toFixed(3)}s</span>
              )}
              {cand.cost != null && (
                <span className="text-muted-foreground text-xs">费用 ${Number(cand.cost).toFixed(6)}</span>
              )}
              {truthy(cand.status) && <StatusBadge status={String(cand.status)} />}
            </div>
          )
        })}
      </div>
    </div>
  )
}

function TurnsTimeline({ turns }: { turns: unknown[] }) {
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium text-muted-foreground">对话轮次（{turns.length}）</p>
      <div className="space-y-2">
        {turns.map((t, i) => {
          const turn = t as Record<string, unknown>
          return (
            <div key={i} className="rounded border p-3 text-sm">
              <div className="flex items-center gap-2 mb-1">
                <span className="text-muted-foreground text-xs">Turn {i + 1}</span>
                {truthy(turn.model_id) && <span className="font-mono text-xs">{String(turn.model_id)}</span>}
                {truthy(turn.status) && <StatusBadge status={String(turn.status)} />}
              </div>
              {truthy(turn.content) && (
                <p className="text-xs whitespace-pre-wrap">{toStr(turn.content)}</p>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

export function TraceView({ strategy, detail }: TraceViewProps) {
  const status = typeof detail.status === 'string' ? detail.status : undefined

  const isPanelStrategy = strategy === 'synthesize' || strategy === 'best_of_n' || strategy === 'best-of-n'
  const isMultimodal = strategy === 'multimodal'

  const hasExtras = !isPanelStrategy && !isMultimodal &&
    !Array.isArray(detail.attempts) && !Array.isArray(detail.candidates) &&
    Object.keys(detail).filter(k => k !== 'status').length > 0

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-3">
          <CardTitle className="text-base">编排 Trace</CardTitle>
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
