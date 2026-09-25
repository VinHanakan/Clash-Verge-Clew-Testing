import { invoke } from '@tauri-apps/api/core'
import { useEffect, useRef, useState } from 'react'

type Entry = { step: string; at: string; value?: string }
type AppProxyStatus = { effective_rules_version?: number; runtime_id?: string }

const formatValue = (value: unknown) => {
  if (typeof value === 'string') return value
  if (value instanceof Error) {
    return `${value.name}: ${value.message}${value.stack ? `\n${value.stack}` : ''}`
  }
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

const enabled =
  import.meta.env.DEV && import.meta.env.VITE_CLEW_PHASE3_STATUS_SELFTEST === '1'
const startEnabled =
  enabled && import.meta.env.VITE_CLEW_PHASE3_START_SELFTEST === '1'
const forwardEnabled =
  startEnabled && import.meta.env.VITE_CLEW_PHASE3_FORWARD_SELFTEST === '1'
const bOnlyEnabled =
  forwardEnabled && import.meta.env.VITE_CLEW_PHASE3_B_ONLY_SELFTEST === '1'
const lifecycleEnabled =
  startEnabled && import.meta.env.VITE_CLEW_PHASE3_LIFECYCLE_SELFTEST === '1'
const lifecycleClientControlUrl =
  import.meta.env.VITE_CLEW_PHASE3_LIFECYCLE_CLIENT_CONTROL_URL
const lifecycleProfileSwitchEnabled =
  lifecycleEnabled && import.meta.env.VITE_CLEW_PHASE3_PROFILE_SWITCH_SELFTEST === '1'
const lifecycleKeepProfile = import.meta.env.VITE_CLEW_PHASE3_KEEP_PROFILE
const lifecycleDropProfile = import.meta.env.VITE_CLEW_PHASE3_DROP_PROFILE
const exitAfterStart =
  startEnabled && import.meta.env.VITE_CLEW_PHASE3_EXIT_AFTER_START === '1'
const appTrafficTelemetryEnabled =
  startEnabled && import.meta.env.VITE_CLEW_APP_TRAFFIC_TELEMETRY === '1'
const clientControlUrl =
  import.meta.env.VITE_CLEW_PHASE3_CLIENT_CONTROL_URL || 'http://127.0.0.1:18182'
const nonTargetControlUrl =
  import.meta.env.VITE_CLEW_PHASE3_NON_TARGET_CONTROL_URL || 'http://127.0.0.1:18183'
const bClientControlUrl =
  import.meta.env.VITE_CLEW_PHASE3_B_CLIENT_CONTROL_URL || clientControlUrl
const fixedStrategyGroup = import.meta.env.VITE_CLEW_PHASE3_FIXED_GROUP
const testRunId = import.meta.env.VITE_CLEW_PHASE3_RUN_ID || 'unset'
const testMode = import.meta.env.VITE_CLEW_PHASE3_TEST_MODE || 'unset'
const diagnosticUrl = import.meta.env.VITE_CLEW_PHASE3_DIAGNOSTIC_URL

export function DevAppProxySelfTest() {
  const startedRef = useRef(false)
  const manualProfileSwitchResolver = useRef<null | (() => void)>(null)
  const [correlationId] = useState(() => (enabled ? crypto.randomUUID() : ''))
  const [entries, setEntries] = useState<Entry[]>([])
  const [manualProfileSwitchReady, setManualProfileSwitchReady] = useState(false)

  useEffect(() => {
    if (!enabled || startedRef.current) return
    startedRef.current = true
    const id = correlationId

    const report = (event: string, value?: unknown) => {
      const payload = {
        event,
        at: new Date().toISOString(),
        runId: testRunId,
        correlationId: id,
        value: value === undefined ? undefined : formatValue(value),
      }
      console.info('[app-proxy-selftest-diagnostic]', payload)
      if (!diagnosticUrl) return
      void fetch(`${diagnosticUrl}/event`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(payload),
      }).catch((error) => console.error('[app-proxy-selftest-diagnostic] report failed', error))
    }

    const onError = (event: ErrorEvent) => report('window.error', { message: event.message, filename: event.filename, line: event.lineno, column: event.colno })
    const onUnhandledRejection = (event: PromiseRejectionEvent) => report('window.unhandledrejection', event.reason)
    window.addEventListener('error', onError)
    window.addEventListener('unhandledrejection', onUnhandledRejection)
    const heartbeat = window.setInterval(() => report('page.heartbeat'), 1000)

    const add = (step: string, value?: unknown) => {
      const entry = { step, at: new Date().toISOString(), value: formatValue(value) }
      setEntries((current) => [...current, entry])
      console.info('[app-proxy-selftest]', { correlationId: id, ...entry })
      report(`selftest.${step}`, value)
    }

    void (async () => {
      add('sequence started')
      add('get_app_uptime started')
      try {
        add('get_app_uptime returned', await invoke<number>('get_app_uptime'))
      } catch (error) {
        add('get_app_uptime failed', error)
      }

      add('get_app_proxy_status started')
      try {
        const status = await invoke('get_app_proxy_status', { correlationId: id })
        add('get_app_proxy_status returned', status)
        const returnedRunId = (status as AppProxyStatus).runtime_id
        add('test context', { runId: testRunId, mode: testMode, strategyGroup: fixedStrategyGroup, backendRunId: returnedRunId })
        if (returnedRunId !== testRunId) throw new Error(`test run mismatch: frontend=${testRunId} backend=${returnedRunId}`)
        if (startEnabled) {
          add('start_app_proxy started')
          try {
            const startedStatus = await invoke('start_app_proxy', {
              request: {
                process_cmdline: (bOnlyEnabled || lifecycleClientControlUrl) ? 'b-client.mjs' : 'runtime-client.mjs',
                strategy_group: 'regular',
                correlation_id: id,
              },
            })
            add('start_app_proxy returned', startedStatus)
            add('get_app_proxy_status after start started')
            report('post-start-status.invoke.started')
            try {
              const afterStartStatus = await invoke('get_app_proxy_status', { correlationId: id })
              report('post-start-status.invoke.succeeded', afterStartStatus)
              add('get_app_proxy_status after start returned', afterStartStatus)
            } catch (error) {
              report('post-start-status.invoke.failed', error)
              throw error
            }
            if (exitAfterStart) {
              add('exit_app started')
              await invoke('exit_app')
              return
            }
            if (appTrafficTelemetryEnabled) {
              add('app_traffic_telemetry sequence started')
              report('telemetry.started')

              // 1. Fetch exact client PID from clientControlUrl /info
              add('query client info started')
              let targetClientInfo: { pid: number; ready: string } | null = null
              for (let i = 0; i < 10; i++) {
                try {
                  const infoRes = await fetch(`${clientControlUrl}/info`)
                  if (infoRes.ok) {
                    targetClientInfo = await infoRes.json()
                    add('query client info returned', targetClientInfo)
                    break
                  }
                } catch (err) {
                  add('query client info attempt error', err)
                }
                await new Promise((r) => setTimeout(r, 500))
              }
              if (!targetClientInfo || !targetClientInfo.pid) {
                throw new Error(`failed to obtain target client PID from ${clientControlUrl}/info`)
              }
              const targetPid = targetClientInfo.pid

              // 2. Trigger active network traffic so a real TCP connection exists for targetPid
              add('client trigger traffic started')
              const probeRes = await fetch(`${clientControlUrl}/trigger?label=telemetry-probe`).then((r) => r.json())
              add('client trigger traffic returned', probeRes)
              if (!probeRes || probeRes.error) {
                throw new Error(`target client probe traffic failed: ${probeRes?.error || 'empty response'}`)
              }
              const probeLocalPort = probeRes.localPort
              const probeRemoteAddr = probeRes.remoteAddress

              // 3. Poll get_app_proxy_process_tree until targetPid is classified or deadline
              add('get_app_proxy_process_tree started')
              let matchedTargetNode: any = null
              let unhijackedSampleNode: any = null
              let fullTree: any[] = []

              for (let attempt = 0; attempt < 8; attempt++) {
                await new Promise((r) => setTimeout(r, 1000))
                try {
                  const tree = await invoke<any[]>('get_app_proxy_process_tree')
                  fullTree = tree || []
                  let foundTarget: any = null
                  let foundUnhijacked: any = null
                  const scan = (nodes: any[]) => {
                    for (const n of nodes) {
                      if (n.pid === targetPid) foundTarget = n
                      else if (!n.hijacked && !foundUnhijacked) foundUnhijacked = n
                      if (n.children) scan(n.children)
                    }
                  }
                  scan(fullTree)
                  if (foundTarget && foundTarget.hijacked) {
                    matchedTargetNode = foundTarget
                    unhijackedSampleNode = foundUnhijacked
                    break
                  }
                } catch (treeErr) {
                  add(`get_app_proxy_process_tree attempt ${attempt} error`, treeErr)
                }
              }

              if (!matchedTargetNode) {
                throw new Error(`target client PID ${targetPid} was not found with hijacked=true in process tree (roots=${fullTree.length})`)
              }
              if (!unhijackedSampleNode) {
                throw new Error('no unhijacked process found in process tree')
              }
              add('get_app_proxy_process_tree verified', {
                targetPid,
                targetNode: matchedTargetNode,
                unhijackedSample: unhijackedSampleNode,
                totalRoots: fullTree.length,
              })
              report('telemetry.process_tree.verified', {
                targetPid,
                targetNode: matchedTargetNode,
                unhijackedSample: unhijackedSampleNode,
                totalRoots: fullTree.length,
              })

              // 4. Query get_app_proxy_process_detail for targetPid
              add('get_app_proxy_process_detail started', { pid: targetPid })
              const detail = await invoke<any>('get_app_proxy_process_detail', { pid: targetPid })
              add('get_app_proxy_process_detail returned', detail)
              if (!detail || detail.pid !== targetPid) {
                throw new Error(`process detail PID mismatch: expected ${targetPid}, got ${detail?.pid}`)
              }
              if (!detail.image_path || typeof detail.image_path !== 'string' || detail.image_path.length === 0) {
                throw new Error(`process detail missing image_path: ${JSON.stringify(detail)}`)
              }
              if (!detail.cmdline || typeof detail.cmdline !== 'string' || detail.cmdline.length === 0) {
                throw new Error(`process detail missing cmdline: ${JSON.stringify(detail)}`)
              }
              report('telemetry.process_detail.verified', detail)

              // 5. Query get_app_proxy_connections and correlate with targetPid tuple
              add('get_app_proxy_connections started')
              const conns = await invoke<any[]>('get_app_proxy_connections')
              add('get_app_proxy_connections returned', { count: conns?.length, sample: conns?.[0] })
              if (!Array.isArray(conns)) {
                throw new Error(`expected connections array, got ${typeof conns}`)
              }
              const targetConns = conns.filter((c) => c.pid === targetPid)
              if (targetConns.length === 0) {
                throw new Error(`no connections found for target client PID ${targetPid} in ${conns.length} total connections`)
              }
              const matchedTupleConn = targetConns.find((c) =>
                (probeLocalPort && c.local_port === probeLocalPort) ||
                (probeRemoteAddr && c.remote_ip === probeRemoteAddr) ||
                c.local_port === 18182
              )
              let tupleVerified = false
              if (matchedTupleConn) {
                if (matchedTupleConn.hijacked === true) {
                  tupleVerified = true
                } else {
                  throw new Error(`matched target tuple connection has hijacked=false (expected true)`)
                }
              }
              add('target connection tuple verified', {
                targetPid,
                probeTuple: { localPort: probeLocalPort, remoteAddr: probeRemoteAddr },
                matchedConnection: matchedTupleConn,
                tupleVerified,
              })
              report('telemetry.connections.verified', {
                count: conns.length,
                targetConnectionsCount: targetConns.length,
                tupleVerified,
                matchedTupleConn: matchedTupleConn || null,
                note: '接管判定仅代表驱动层流量拦截判定，不等于端到端代理转发成功',
              })

              report('telemetry.succeeded', {
                targetPid,
                matchedTargetNode,
                unhijackedSampleNode,
                detail,
                connectionCount: conns.length,
                targetConnectionsCount: targetConns.length,
                matchedTupleConn,
              })
              // Proceed to finally where stop_app_proxy is called
              return
            }
            if (lifecycleEnabled) {
              const runtimeConfig = (value: unknown) => {
                if (!value || typeof value !== 'object') return value
                const listeners = Array.isArray((value as { listeners?: unknown }).listeners)
                  ? (value as { listeners: unknown[] }).listeners
                  : []
                return { managedListenerCount: listeners.filter((item) =>
                  typeof item === 'object' && item !== null &&
                  (item as { name?: unknown }).name === 'clash-verge-app-proxy',
                ).length, listeners }
              }
              add('lifecycle runtime config before reload started')
              add('lifecycle runtime config before reload returned', runtimeConfig(await invoke('get_runtime_config')))
              add('lifecycle reload started')
              add('lifecycle reload returned', await invoke('update_proxy_chain_config_in_runtime', { proxyChainConfig: null }))
              add('lifecycle runtime config after reload returned', runtimeConfig(await invoke('get_runtime_config')))
              if (lifecycleClientControlUrl) {
                add('lifecycle client after reload started')
                add('lifecycle client after reload returned', await fetch(`${lifecycleClientControlUrl}/trigger?label=after-reload`).then((response) => response.json()))
              }
              if (lifecycleProfileSwitchEnabled) {
                add('lifecycle profile switch waiting for explicit action')
                await new Promise<void>((resolve) => {
                  manualProfileSwitchResolver.current = resolve
                  setManualProfileSwitchReady(true)
                })
                manualProfileSwitchResolver.current = null
                setManualProfileSwitchReady(false)
                if (!lifecycleKeepProfile || !lifecycleDropProfile) throw new Error('profile switch self-test requires keep and drop profile IDs')
                add(`lifecycle switch to ${lifecycleKeepProfile} started`)
                add(`lifecycle switch to ${lifecycleKeepProfile} returned`, await invoke('patch_profiles_config_by_profile_index', { profileIndex: lifecycleKeepProfile, correlationId: id }))
                add(`lifecycle status after ${lifecycleKeepProfile} returned`, await invoke('get_app_proxy_status', { correlationId: id }))
                if (lifecycleClientControlUrl) {
                  add(`lifecycle client after ${lifecycleKeepProfile} started`)
                  add(`lifecycle client after ${lifecycleKeepProfile} returned`, await fetch(`${lifecycleClientControlUrl}/trigger?label=after-keep-profile`).then((response) => response.json()))
                }
                add(`lifecycle switch to ${lifecycleDropProfile} started`)
                add(`lifecycle switch to ${lifecycleDropProfile} returned`, await invoke('patch_profiles_config_by_profile_index', { profileIndex: lifecycleDropProfile, correlationId: id }))
                add(`lifecycle status after ${lifecycleDropProfile} returned`, await invoke('get_app_proxy_status', { correlationId: id }))
                try {
                  await invoke('set_app_proxy_strategy', {
                    strategyGroup: fixedStrategyGroup,
                    strategy_group: fixedStrategyGroup,
                    correlationId: id,
                  })
                  add(`lifecycle missing group ${fixedStrategyGroup} unexpectedly returned`)
                } catch (error) {
                  add(`lifecycle missing group ${fixedStrategyGroup} failed as expected`, error)
                }
                add(`lifecycle status after missing group returned`, await invoke('get_app_proxy_status', { correlationId: id }))
              }
              add('lifecycle invalid strategy started')
              try {
                await invoke('set_app_proxy_strategy', {
                  strategyGroup: '__CLEW_PHASE3_NONEXISTENT__',
                  strategy_group: '__CLEW_PHASE3_NONEXISTENT__',
                  correlationId: id,
                })
                add('lifecycle invalid strategy unexpectedly returned')
              } catch (error) {
                add('lifecycle invalid strategy failed as expected', error)
              }
              add('lifecycle status after invalid strategy returned', await invoke('get_app_proxy_status', { correlationId: id }))
              if (import.meta.env.VITE_CLEW_PHASE3_LIFECYCLE_SKIP_RESTART !== '1') {
                add('lifecycle Mihomo restart started')
                add('lifecycle Mihomo restart returned', await invoke('restart_core'))
                add('lifecycle status after Mihomo restart returned', await invoke('get_app_proxy_status', { correlationId: id }))
                if (lifecycleClientControlUrl) {
                  add('lifecycle client after Mihomo restart started')
                  add('lifecycle client after Mihomo restart returned', await fetch(`${lifecycleClientControlUrl}/trigger?label=after-restart`).then((response) => response.json()))
                }
              }
              return
            }
            if (forwardEnabled) {
              if (!fixedStrategyGroup) throw new Error('VITE_CLEW_PHASE3_FIXED_GROUP is required for the specified-group self-test')
              add(`set_app_proxy_strategy ${fixedStrategyGroup} started`)
              const switchedStatus = await invoke<AppProxyStatus>('set_app_proxy_strategy', {
                strategyGroup: fixedStrategyGroup,
                strategy_group: fixedStrategyGroup,
                correlationId: id,
              })
              add(`set_app_proxy_strategy ${fixedStrategyGroup} returned`, switchedStatus)
              if (bOnlyEnabled) {
                if (switchedStatus.effective_rules_version !== 2) {
                  throw new Error(`expected effective rules version 2, got ${formatValue(switchedStatus)}`)
                }
                add('get_app_proxy_status after B started')
                const bStatus = await invoke<AppProxyStatus>('get_app_proxy_status', { correlationId: id })
                add('get_app_proxy_status after B returned', bStatus)
                if (bStatus.effective_rules_version !== 2) {
                  throw new Error(`expected confirmed effective rules version 2, got ${formatValue(bStatus)}`)
                }
                add('client B started')
                add('client B returned', await fetch(`${bClientControlUrl}/trigger?label=B`).then((response) => response.json()))
                return
              }
              add('client direct baseline started')
              add('client direct baseline returned', await fetch(`${clientControlUrl}/trigger?label=direct`).then((response) => response.json()))
              add('client A started')
              add('client A returned', await fetch(`${clientControlUrl}/trigger?label=A`).then((response) => response.json()))
              add('non-target started')
              add('non-target returned', await fetch(`${nonTargetControlUrl}/trigger`).then((response) => response.json()))
              add('client B started')
              add('client B returned', await fetch(`${clientControlUrl}/trigger?label=B`).then((response) => response.json()))
            }
          } catch (error) {
            add('start_app_proxy failed', error)
          } finally {
            add('stop_app_proxy started')
            report('stop.invoke.started')
            try {
              const stoppedStatus = await invoke<AppProxyStatus>('stop_app_proxy', { correlationId: id })
              if (!stoppedStatus || (stoppedStatus as any).state !== 'stopped') {
                throw new Error(`stop_app_proxy returned unexpected state: ${(stoppedStatus as any)?.state}`)
              }
              report('stop.invoke.succeeded', stoppedStatus)
              add('stop_app_proxy returned', stoppedStatus)
              if (forwardEnabled) {
                add('client after-stop started')
                add('client after-stop returned', await fetch(`${clientControlUrl}/trigger?label=after-stop`).then((response) => response.json()))
              }
            } catch (error) {
              report('stop.invoke.failed', error)
              add('stop_app_proxy failed', error)
            }
            if (import.meta.env.VITE_CLEW_PHASE3_RUNTIME_EVIDENCE === '1') {
              try {
                await invoke('get_runtime_config')
              } catch (error) {
                add('runtime config after stop failed', error)
              }
            }
            if (appTrafficTelemetryEnabled) {
              add('exit_app after telemetry started')
              await invoke('exit_app')
              return
            }
          }
        }
      } catch (error) {
        add('get_app_proxy_status failed', error)
      }
    })()

    return () => {
      window.clearInterval(heartbeat)
      window.removeEventListener('error', onError)
      window.removeEventListener('unhandledrejection', onUnhandledRejection)
      report('page.unmounted')
    }
  }, [correlationId])

  if (!enabled) return null

  return (
    <aside
      data-testid="dev-app-proxy-selftest"
      style={{
        position: 'fixed', zIndex: 2147483647, top: 12, right: 12,
        width: 460, maxHeight: '80vh', overflow: 'auto', padding: 12,
        color: '#111', background: '#fff8d8', border: '1px solid #9b7b00',
        borderRadius: 6, font: '12px/1.4 monospace', whiteSpace: 'pre-wrap',
      }}
    >
      <strong>AppProxy development self-test</strong>
      <div>run ID: {testRunId}</div>
      <div>mode: {testMode}; target group: {fixedStrategyGroup || 'unset'}</div>
      <div>correlation ID: {correlationId}</div>
      {manualProfileSwitchReady && (
        <button
          type="button"
          onClick={() => manualProfileSwitchResolver.current?.()}
          style={{ margin: '8px 0', padding: '4px 8px' }}
        >
          Run profile-switch lifecycle test
        </button>
      )}
      {entries.map((entry) => (
        <div key={`${entry.at}-${entry.step}`}>
          <div>{`${entry.at} ${entry.step}`}</div>
          {entry.value && <div>{entry.value}</div>}
        </div>
      ))}
    </aside>
  )
}
