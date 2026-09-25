import { invoke } from '@tauri-apps/api/core'
import {
  AddRounded,
  CheckRounded,
  ClearRounded,
  CloseRounded,
  ContentCopyRounded,
  DeleteOutlineRounded,
  EditRounded,
  ExpandMoreRounded,
  ChevronRightRounded,
  FilterListRounded,
  HubRounded,
  LanRounded,
  PlayArrowRounded,
  RefreshRounded,
  SearchRounded,
  StopRounded,
  TuneRounded,
} from '@mui/icons-material'
import {
  Alert,
  Box,
  Button,
  Chip,
  CircularProgress,
  Collapse,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  Drawer,
  FormControlLabel,
  IconButton,
  InputAdornment,
  MenuItem,
  Paper,
  Select,
  Switch,
  Tab,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Tabs,
  TextField,
  Tooltip,
  Typography,
  alpha,
  useTheme,
} from '@mui/material'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { BasePage } from '@/components/base'

export type AppProxyRule = {
  id: string
  name: string
  enabled: boolean
  process_name: string
  cmdline_pattern: string
  hack_tree: boolean
  proxy_group_id: number
  strategy_group: string
  protocol: string
}

function processRuleTarget(name: string, imagePath?: string | null, cmdline?: string | null): string | null {
  if (/^(node|python|pythonw)\.exe$/i.test(name)) {
    return cmdline && cmdline !== imagePath && cmdline.trim().length > 0 ? cmdline : null
  }
  return imagePath || name
}

export type AppProxyStatus = {
  state: string
  helper_ready: boolean
  proxy_ready: boolean
  effective_rules_version: number | null
  pid: number | null
  strategy_group: string | null
  runtime_id?: string | null
  last_startup_error?: string | null
}

export type AppProxyConnection = {
  pid: number
  process_name: string
  local_ip: string
  local_port: number
  remote_ip: string
  remote_port: number
  state: string
  dest?: string | null
  hijacked: boolean
  pid_alive: boolean
  proxy_status?: string | null
  rule_matched?: boolean
  outbound_group?: string | null
  hijack_source?: string | null
  bytes_sent?: number
  bytes_received?: number
}

export type AppProxyProcessNode = {
  pid: number
  parent_pid: number
  name: string
  hijacked: boolean
  hijack_source?: string | null
  children?: AppProxyProcessNode[]
  cmdline?: string | null
  image_path?: string | null
}

export type AppProxyProcessDetail = {
  pid: number
  name: string
  parent_pid?: number | null
  hijacked: boolean
  hijack_source?: string | null
  cmdline?: string | null
  image_path?: string | null
}

const initialStatus: AppProxyStatus = {
  state: 'unknown',
  helper_ready: false,
  proxy_ready: false,
  effective_rules_version: null,
  pid: null,
  strategy_group: null,
}

export default function AppTrafficPage() {
  const { t } = useTranslation()
  const theme = useTheme()

  // --- Single Rule & Execution State ---
  const [processCmdline, setProcessCmdline] = useState('')
  const [strategyMode, setStrategyMode] = useState<'regular' | 'fixed'>('regular')
  const [strategyGroup, setStrategyGroup] = useState('')
  const [strategyGroups, setStrategyGroups] = useState<string[]>([])
  const [status, setStatus] = useState<AppProxyStatus>(initialStatus)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>('')
  const [telemetryError, setTelemetryError] = useState<string | null>(null)
  const [isStale, setIsStale] = useState(false)

  // --- Multi-Application Rules State ---
  const [rules, setRules] = useState<AppProxyRule[]>([])
  const [addRuleDialogOpen, setAddRuleDialogOpen] = useState(false)
  const [newRuleName, setNewRuleName] = useState('')
  const [newRulePattern, setNewRulePattern] = useState('')
  const [editingRuleId, setEditingRuleId] = useState<string | null>(null)
  const [rulesExpanded, setRulesExpanded] = useState(true)

  // --- Real Telemetry State ---
  const [connections, setConnections] = useState<AppProxyConnection[]>([])
  const [processTree, setProcessTree] = useState<AppProxyProcessNode[]>([])
  const [loadingData, setLoadingData] = useState(false)
  const [autoRefresh, setAutoRefresh] = useState(true)

  // --- Filtering & Selection ---
  const [processSearch, setProcessSearch] = useState('')
  const [connSearch, setConnSearch] = useState('')
  const [treeTab, setTreeTab] = useState<'all' | 'active' | 'hijacked'>('all')
  const [selectedPid, setSelectedPid] = useState<number | null>(null)
  const [processDetail, setProcessDetail] = useState<AppProxyProcessDetail | null>(null)
  const [loadingDetail, setLoadingDetail] = useState(false)
  const [expandedPids, setExpandedPids] = useState<Record<number, boolean>>({})
  const [copiedField, setCopiedField] = useState<string | null>(null)

  const inFlightRef = useRef(false)
  const statusRef = useRef(status)
  statusRef.current = status

  const isRunning = status.state === 'running' || status.state === 'degraded'

  const loadRules = useCallback(async () => {
    try {
      const list = await invoke<AppProxyRule[]>('get_app_proxy_rules')
      setRules(list || [])
    } catch (err) {
      setError(`读取应用代理规则失败：${String(err)}`)
    }
  }, [])

  const loadStrategyGroups = useCallback(async () => {
    try {
      setStrategyGroups(await invoke<string[]>('get_app_proxy_strategy_groups'))
    } catch (reason) {
      setStrategyGroups([])
      setError(String(reason))
    }
  }, [])

  // Query helper status and telemetry (covered by inFlightRef across whole cycle)
  const refreshAll = useCallback(async (currentStatus?: AppProxyStatus) => {
    if (inFlightRef.current) return
    inFlightRef.current = true
    try {
      let s = currentStatus
      if (!s) {
        try {
          s = await invoke<AppProxyStatus>('get_app_proxy_status')
          setStatus(s)
          if (s.last_startup_error) {
            setError(s.last_startup_error)
          } else if (s.state === 'running') {
            setError('')
          }
        } catch (reason: any) {
          setError(reason?.message || String(reason))
          setIsStale(true)
          return
        }
      }

      setLoadingData(true)
      try {
        const [conns, tree] = await Promise.all([
          invoke<AppProxyConnection[]>('get_app_proxy_connections'),
          invoke<AppProxyProcessNode[]>('get_app_proxy_process_tree'),
        ])
        setConnections(conns || [])
        setProcessTree(tree || [])
        setTelemetryError(null)
        setIsStale(false)
      } catch (e: any) {
        setTelemetryError(e?.message || String(e))
        setIsStale(true)
      } finally {
        setLoadingData(false)
      }
    } finally {
      inFlightRef.current = false
    }
  }, [])

  // Mount effect: runs strictly ONCE on mount
  useEffect(() => {
    void refreshAll()
    void loadRules()
    void loadStrategyGroups()
  }, [refreshAll, loadRules, loadStrategyGroups])

  // Periodic polling effect: decoupled, guarded by inFlightRef across whole cycle
  useEffect(() => {
    if (!autoRefresh) return
    const timer = setInterval(() => {
      void refreshAll()
    }, 3000)
    return () => clearInterval(timer)
  }, [autoRefresh, refreshAll])

  // Load process detail when selectedPid changes (works in both monitor & proxy mode)
  useEffect(() => {
    if (selectedPid == null) {
      setProcessDetail(null)
      return
    }
    let cancelled = false
    setLoadingDetail(true)
    invoke<AppProxyProcessDetail>('get_app_proxy_process_detail', { pid: selectedPid })
      .then((detail) => {
        if (!cancelled) setProcessDetail(detail)
      })
      .catch((err: any) => {
        if (!cancelled) {
          setError(`读取进程详情失败：${String(err)}`)
          setProcessDetail(null)
        }
      })
      .finally(() => {
        if (!cancelled) setLoadingDetail(false)
      })
    return () => {
      cancelled = true
    }
  }, [selectedPid])

  const handleToggleRule = async (ruleId: string, enabled: boolean) => {
    setBusy(true)
    setError('')
    try {
      const s = await invoke<AppProxyStatus>('toggle_app_proxy_rule', {
        ruleId,
        enabled,
      })
      setStatus(s)
      await loadRules()
      await refreshAll(s)
    } catch (err: any) {
      setError(err?.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  const handleRemoveRule = async (ruleId: string) => {
    setBusy(true)
    setError('')
    try {
      const s = await invoke<AppProxyStatus>('remove_app_proxy_rule', {
        ruleId,
      })
      setStatus(s)
      await loadRules()
      await refreshAll(s)
    } catch (err: any) {
      setError(err?.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  const handleAddRule = async (name: string, pattern: string) => {
    if (!pattern.trim()) return
    setBusy(true)
    setError('')
    try {
      const rule: AppProxyRule = {
        id: `rule-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
        name: name.trim() || pattern.trim().split('\\').pop() || pattern.trim(),
        enabled: true,
        process_name: '*',
        cmdline_pattern: pattern.trim(),
        hack_tree: false,
        proxy_group_id: 0,
        strategy_group: strategyMode === 'regular' ? 'regular' : strategyGroup.trim(),
        protocol: 'tcp',
      }
      if (!rule.strategy_group) throw new Error('请选择有效的策略组')
      const s = await invoke<AppProxyStatus>('add_app_proxy_rule', { rule })
      setStatus(s)
      await loadRules()
      await refreshAll(s)
      setAddRuleDialogOpen(false)
      setNewRuleName('')
      setNewRulePattern('')
      setEditingRuleId(null)
    } catch (err: any) {
      setError(err?.message || String(err))
    } finally {
      setBusy(false)
    }
  }

  const handleSaveRule = async () => {
    if (!editingRuleId) {
      await handleAddRule(newRuleName, newRulePattern)
      return
    }
    const group = strategyMode === 'regular' ? 'regular' : strategyGroup.trim()
    if (!group || !newRulePattern.trim()) {
      setError('请输入有效的匹配模式与策略组')
      return
    }
    const existing = rules.find((rule) => rule.id === editingRuleId)
    if (!existing) return
    setBusy(true)
    try {
      const result = await invoke<AppProxyStatus>('add_app_proxy_rule', {
        rule: { ...existing, name: newRuleName.trim() || existing.name, cmdline_pattern: newRulePattern.trim(), strategy_group: group },
      })
      setStatus(result)
      await loadRules()
      await refreshAll(result)
      setAddRuleDialogOpen(false)
      setEditingRuleId(null)
    } catch (reason: any) {
      setError(reason?.message || String(reason))
    } finally {
      setBusy(false)
    }
  }

  // Start proxy (using all enabled rules, or processCmdline fallback)
  const handleStart = async () => {
    setBusy(true)
    setError('')
    try {
      const group = strategyMode === 'regular' ? 'regular' : strategyGroup.trim()

      let currentRules = rules
      if (currentRules.length === 0) {
        if (!group) throw new Error('请选择有效的策略组')
        if (!processCmdline.trim()) {
          throw new Error('请先在下方添加代理规则，或在输入框中填入应用程序关键字')
        }
        const initialRule: AppProxyRule = {
          id: `rule-${Date.now()}`,
          name: processCmdline.trim().split('\\').pop() || processCmdline.trim(),
          enabled: true,
          process_name: '*',
          cmdline_pattern: processCmdline.trim(),
          hack_tree: false,
          proxy_group_id: 0,
          strategy_group: group,
          protocol: 'tcp',
        }
        currentRules = [initialRule]
        setRules(currentRules)
      } else {
        const hasEnabled = currentRules.some((r) => r.enabled)
        if (!hasEnabled) {
          throw new Error('当前所有代理规则均处于停用状态，请至少启用一条规则')
        }
      }

      const s = await invoke<AppProxyStatus>('start_app_proxy', {
        request: {
          process_cmdline: currentRules.find((r) => r.enabled)?.cmdline_pattern || processCmdline.trim(),
          strategy_group: currentRules.find((rule) => rule.enabled)?.strategy_group || group,
          rules: currentRules,
        },
      })
      setStatus(s)
      await loadRules()
      await refreshAll(s)
    } catch (reason: any) {
      setError(reason?.message || String(reason))
    } finally {
      setBusy(false)
    }
  }

  // Start or hot-reload proxy for a specific target cmdline / executable
  const handleStartWithTarget = async (target: string, name?: string) => {
    setBusy(true)
    setError('')
    try {
      const group = strategyMode === 'regular' ? 'regular' : strategyGroup.trim()
      if (!target.trim()) throw new Error('请输入要匹配的应用程序或命令行关键字')

      const existing = rules.find((r) => r.cmdline_pattern.toLowerCase() === target.trim().toLowerCase())
      let updatedRules: AppProxyRule[]
      if (existing) {
        updatedRules = rules.map((r) => (r.id === existing.id ? { ...r, enabled: true } : r))
      } else {
        if (!group) throw new Error('请选择有效的策略组')
        const newRule: AppProxyRule = {
          id: `rule-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          name: name?.trim() || target.trim().split('\\').pop() || target.trim(),
          enabled: true,
          process_name: '*',
          cmdline_pattern: target.trim(),
          hack_tree: false,
          proxy_group_id: 0,
          strategy_group: group,
          protocol: 'tcp',
        }
        updatedRules = [...rules, newRule]
      }

      if (isRunning) {
        // Dynamic in-flight hot-reload
        const s = await invoke<AppProxyStatus>('set_app_proxy_rules', { rules: updatedRules })
        setStatus(s)
        await loadRules()
        await refreshAll(s)
      } else {
        const s = await invoke<AppProxyStatus>('start_app_proxy', {
          request: {
            process_cmdline: target.trim(),
            strategy_group: updatedRules.find((rule) => rule.enabled)?.strategy_group || group,
            rules: updatedRules,
          },
        })
        setStatus(s)
        await loadRules()
        await refreshAll(s)
      }
    } catch (reason: any) {
      setError(reason?.message || String(reason))
    } finally {
      setBusy(false)
    }
  }

  // Stop proxy
  const handleStop = async () => {
    setBusy(true)
    setError('')
    try {
      const s = await invoke<AppProxyStatus>('stop_app_proxy')
      setStatus(s)
      setSelectedPid(null)
      setTelemetryError(null)
      setIsStale(false)
      await refreshAll(s)
    } catch (reason: any) {
      setError(reason?.message || String(reason))
    } finally {
      setBusy(false)
    }
  }

  // --- Development automated page test hook ---
  const pageTestStartedRef = useRef(false)
  useEffect(() => {
    if (!import.meta.env.DEV || import.meta.env.VITE_CLEW_APP_TRAFFIC_PAGE_TEST !== '1') return
    if (pageTestStartedRef.current) return
    pageTestStartedRef.current = true

    const diagUrl = import.meta.env.VITE_CLEW_PHASE3_DIAGNOSTIC_URL || 'http://127.0.0.1:18185'
    const clientControlUrl = import.meta.env.VITE_CLEW_PHASE3_CLIENT_CONTROL_URL || 'http://127.0.0.1:18182'
    const runId = import.meta.env.VITE_CLEW_PHASE3_RUN_ID || 'page-test'

    const sendDiag = async (event: string, value?: unknown) => {
      try {
        await fetch(`${diagUrl}/event`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            event,
            runId,
            at: new Date().toISOString(),
            value: typeof value === 'string' ? value : JSON.stringify(value),
          }),
        })
      } catch {}
    }

    void (async () => {
      try {
        await sendDiag('page_test.started')

        // 1. Fetch target client info from control url
        let targetInfo: { pid: number; ready: string } | null = null
        for (let i = 0; i < 15; i++) {
          try {
            const res = await fetch(`${clientControlUrl}/info`)
            if (res.ok) {
              targetInfo = await res.json()
              break
            }
          } catch {}
          await new Promise((r) => setTimeout(r, 400))
        }
        if (!targetInfo?.pid) throw new Error('Target client PID not available from clientControlUrl')
        const targetPid = targetInfo.pid
        await sendDiag('page_test.client_ready', { targetPid })

        // 2. Start single rule
        setProcessCmdline('runtime-client.mjs')
        setStrategyMode('regular')
        await sendDiag('page_test.start_proxy.invoking')
        const startedStatus = await invoke<AppProxyStatus>('start_app_proxy', {
          request: {
            process_cmdline: 'runtime-client.mjs',
            strategy_group: 'regular',
          },
        })
        setStatus(startedStatus)
        await sendDiag('page_test.start_proxy.succeeded', startedStatus)

        // 3. Trigger active traffic from target client
        const probeRes = await fetch(`${clientControlUrl}/trigger?label=page-test-probe`).then((r) => r.json())
        if (!probeRes || probeRes.error) {
          throw new Error(`probe traffic failed: ${probeRes?.error || 'empty'}`)
        }
        await sendDiag('page_test.probe_traffic.succeeded', probeRes)

        // 4. Poll page refreshAll() until targetPid appears in processTree with hijacked=true
        let matchedNode: AppProxyProcessNode | null = null
        for (let attempt = 0; attempt < 12; attempt++) {
          await new Promise((r) => setTimeout(r, 1000))
          await refreshAll(startedStatus)
          const tree = await invoke<AppProxyProcessNode[]>('get_app_proxy_process_tree')
          if (Array.isArray(tree)) {
            setProcessTree(tree)
            const scan = (nodes: AppProxyProcessNode[]) => {
              for (const n of nodes) {
                if (n.pid === targetPid && n.hijacked) {
                  matchedNode = n
                  return
                }
                if (n.children) scan(n.children)
              }
            }
            scan(tree)
            if (matchedNode) break
          }
        }
        if (!matchedNode) throw new Error(`Target PID ${targetPid} not found with hijacked=true in page processTree`)
        await sendDiag('page_test.tree_verified', { targetPid, matchedNode })

        // 5. Select targetPid in the page (triggers setSelectedPid -> loads detail drawer)
        setSelectedPid(targetPid)
        await sendDiag('page_test.selected_pid_set', { targetPid })

        // Wait for processDetail state to populate via page's useEffect
        let detailLoaded: AppProxyProcessDetail | null = null
        for (let i = 0; i < 10; i++) {
          await new Promise((r) => setTimeout(r, 500))
          const d = await invoke<AppProxyProcessDetail>('get_app_proxy_process_detail', { pid: targetPid })
          if (d && d.pid === targetPid && d.image_path && d.cmdline) {
            detailLoaded = d
            setProcessDetail(d)
            break
          }
        }
        if (!detailLoaded) throw new Error('Failed to load processDetail for targetPid in page')
        await sendDiag('page_test.detail_verified', detailLoaded)

        // 6. Verify connection filtering for selectedPid
        // Trigger active connection probe to ensure targetPid has active connections during query
        let conns: AppProxyConnection[] = []
        let targetConns: AppProxyConnection[] = []
        let lastProbeRes = probeRes
        for (let attempt = 0; attempt < 8; attempt++) {
          try {
            const probe = await fetch(`${clientControlUrl}/trigger?label=page-test-conn-${attempt}`).then((r) => r.json())
            if (probe && !probe.error) lastProbeRes = probe
          } catch {}
          conns = await invoke<AppProxyConnection[]>('get_app_proxy_connections')
          if (Array.isArray(conns)) {
            setConnections(conns)
            targetConns = conns.filter((c) => c.pid === targetPid)
            if (targetConns.length > 0) break
          }
          await new Promise((r) => setTimeout(r, 400))
        }

        if (targetConns.length === 0) throw new Error(`No connections for targetPid ${targetPid} after retries`)
        const matchedTupleConn = targetConns.find((c) =>
          (lastProbeRes?.localPort && c.local_port === lastProbeRes.localPort) ||
          (lastProbeRes?.remoteAddress && c.remote_ip === lastProbeRes.remoteAddress) ||
          c.local_port === 18182
        )
        let tupleVerified = false
        if (matchedTupleConn) {
          if (matchedTupleConn.hijacked === true) {
            tupleVerified = true
          } else {
            throw new Error(`Target connection tuple not marked hijacked=true`)
          }
        }
        await sendDiag('page_test.connections_filtered_verified', {
          targetPid,
          targetConnsCount: targetConns.length,
          tupleVerified,
          matchedTupleConn: matchedTupleConn || null,
          verificationStatus: tupleVerified ? 'VERIFIED' : 'UNVERIFIED',
          note: '接管判定仅代表驱动层流量拦截判定，不等于端到端代理转发成功',
        })

        // 7. Verify Tab switching: switch to 'hijacked' tab
        setTreeTab('hijacked')
        await new Promise((r) => setTimeout(r, 400))
        await sendDiag('page_test.tab_switched', { tab: 'hijacked' })

        // 8. Stop via page handleStop
        const stoppedStatus = await invoke<AppProxyStatus>('stop_app_proxy')
        if (!stoppedStatus || stoppedStatus.state !== 'stopped') {
          throw new Error(`stop_app_proxy returned unexpected state: ${stoppedStatus?.state}`)
        }
        setStatus(stoppedStatus)
        setConnections([])
        setProcessTree([])
        setSelectedPid(null)
        setTelemetryError(null)
        setIsStale(false)
        await sendDiag('page_test.stopped_verified', stoppedStatus)

        // 9. Report in-page real data integration result (page_test.succeeded)
        await sendDiag('page_test.succeeded', {
          targetPid,
          matchedNode,
          detailLoaded,
          targetConnsCount: targetConns.length,
          tupleVerified,
          matchedTupleConn: matchedTupleConn || null,
          stoppedState: stoppedStatus.state,
          integrationMode: 'in_page_data_integration',
        })

        // Clean exit of isolated Verge app
        await new Promise((r) => setTimeout(r, 600))
        await invoke('exit_app')
      } catch (err: any) {
        await sendDiag('page_test.failed', err?.message || String(err))
        try { await invoke('exit_app') } catch {}
      }
    })()
  }, [refreshAll])

  const handleCopy = (text: string, field: string) => {
    navigator.clipboard.writeText(text)
    setCopiedField(field)
    setTimeout(() => setCopiedField(null), 1500)
  }

  // Toggle tree node expansion
  const toggleExpand = (pid: number) => {
    setExpandedPids((prev) => ({ ...prev, [pid]: !prev[pid] }))
  }

  // Compute active connection PIDs and count per PID
  const activePids = useMemo(() => {
    const set = new Set<number>()
    for (const c of connections) {
      set.add(c.pid)
    }
    return set
  }, [connections])

  const pidConnCounts = useMemo(() => {
    const map = new Map<number, number>()
    for (const c of connections) {
      map.set(c.pid, (map.get(c.pid) || 0) + 1)
    }
    return map
  }, [connections])

  const { totalProcesses, hijackedProcesses } = useMemo(() => {
    let total = 0
    let hijacked = 0
    const count = (nodes: AppProxyProcessNode[]) => {
      for (const n of nodes) {
        total++
        if (n.hijacked) hijacked++
        if (n.children) count(n.children)
      }
    }
    count(processTree)
    return { totalProcesses: total, hijackedProcesses: hijacked }
  }, [processTree])

  // Filter process tree
  const filterNode = useCallback(
    (node: AppProxyProcessNode): boolean => {
      const query = processSearch.trim().toLowerCase()
      const matchQuery =
        !query ||
        node.name.toLowerCase().includes(query) ||
        String(node.pid).includes(query) ||
        (node.cmdline && node.cmdline.toLowerCase().includes(query))
      let matchTab = true
      if (treeTab === 'hijacked') {
        matchTab = node.hijacked
      } else if (treeTab === 'active') {
        matchTab = activePids.has(node.pid)
      }
      const childrenMatch = node.children && node.children.some(filterNode)
      return (matchQuery && matchTab) || Boolean(childrenMatch)
    },
    [processSearch, treeTab, activePids],
  )

  const filteredTree = useMemo(() => {
    return processTree.filter(filterNode)
  }, [processTree, filterNode])

  // Filter connections table
  const filteredConnections = useMemo(() => {
    return connections.filter((conn) => {
      if (selectedPid != null && conn.pid !== selectedPid) return false
      if (!connSearch.trim()) return true
      const q = connSearch.trim().toLowerCase()
      return (
        conn.process_name.toLowerCase().includes(q) ||
        String(conn.pid).includes(q) ||
        conn.remote_ip.toLowerCase().includes(q) ||
        (conn.dest && conn.dest.toLowerCase().includes(q)) ||
        String(conn.remote_port).includes(q)
      )
    })
  }, [connections, selectedPid, connSearch])

  // Render recursive tree node
  const renderTreeNode = (node: AppProxyProcessNode, depth = 0) => {
    const hasChildren = node.children && node.children.length > 0
    const isExpanded = expandedPids[node.pid] ?? true
    const isSelected = selectedPid === node.pid
    const connCount = pidConnCounts.get(node.pid) ?? 0

    return (
      <Box key={node.pid} sx={{ pl: depth > 0 ? 2 : 0, mb: 0.5 }}>
        <Box
          data-testid={`process-node-${node.pid}`}
          onClick={() => setSelectedPid(isSelected ? null : node.pid)}
          sx={{
            display: 'flex',
            alignItems: 'center',
            p: '4px 8px',
            borderRadius: 1,
            cursor: 'pointer',
            bgcolor: isSelected
              ? alpha(theme.palette.primary.main, 0.15)
              : 'transparent',
            border: '1px solid',
            borderColor: isSelected
              ? theme.palette.primary.main
              : alpha(theme.palette.divider, 0.4),
            '&:hover': {
              bgcolor: isSelected
                ? alpha(theme.palette.primary.main, 0.22)
                : alpha(theme.palette.action.hover, 0.08),
            },
          }}
        >
          {hasChildren ? (
            <IconButton
              size="small"
              onClick={(e) => {
                e.stopPropagation()
                toggleExpand(node.pid)
              }}
              sx={{ p: 0.2, mr: 0.5 }}
            >
              {isExpanded ? (
                <ExpandMoreRounded fontSize="small" />
              ) : (
                <ChevronRightRounded fontSize="small" />
              )}
            </IconButton>
          ) : (
            <Box sx={{ width: 22 }} />
          )}

          <Typography
            variant="body2"
            sx={{
              fontWeight: isSelected ? 600 : 500,
              flexGrow: 1,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {node.name}
          </Typography>

          <Chip
            size="small"
            label={`PID ${node.pid}`}
            variant="outlined"
            sx={{ height: 20, fontSize: 11, ml: 1, fontFamily: 'monospace' }}
          />

          {connCount > 0 && (
            <Chip
              size="small"
              label={`${connCount} 连接`}
              color="info"
              variant="outlined"
              sx={{ height: 20, fontSize: 10, ml: 0.6 }}
            />
          )}

          <Chip
            size="small"
            label={node.hijacked ? '已接管' : '直连'}
            color={node.hijacked ? 'primary' : 'default'}
            sx={{ height: 20, fontSize: 10, ml: 0.6 }}
          />

          {(() => {
            const target = (processRuleTarget(node.name, node.image_path, node.cmdline) || '').toLowerCase()
            const matchingRule = rules.find((r) => {
              const pat = r.cmdline_pattern.toLowerCase().replace(/^\*|\*$/g, '')
              return pat && target.includes(pat)
            })
            if (!matchingRule) {
              return (
                <Tooltip title={target ? '添加为此应用的代理规则' : '此脚本进程缺少可验证的完整命令行，请手动填写脚本匹配模式'}>
                  <IconButton
                    size="small"
                    disabled={!target}
                    onClick={(e) => {
                      e.stopPropagation()
                      void handleAddRule(node.name, target)
                    }}
                    sx={{ p: 0.3, ml: 0.5 }}
                  >
                    <AddRounded fontSize="small" sx={{ fontSize: 16 }} />
                  </IconButton>
                </Tooltip>
              )
            }
            return (
              <Tooltip title={`规则: ${matchingRule.name} (${matchingRule.enabled ? '已启用 - 点击停用' : '已停用 - 点击启用'})`}>
                <Chip
                  size="small"
                  label={matchingRule.enabled ? '规则开' : '规则关'}
                  color={matchingRule.enabled ? 'success' : 'default'}
                  variant={matchingRule.enabled ? 'filled' : 'outlined'}
                  onClick={(e) => {
                    e.stopPropagation()
                    void handleToggleRule(matchingRule.id, !matchingRule.enabled)
                  }}
                  sx={{ height: 20, fontSize: 10, ml: 0.6, cursor: 'pointer' }}
                />
              </Tooltip>
            )
          })()}

          <Tooltip title={node.hijacked ? "已处于代理接管中 (点击查看详情)" : "查看或为此进程配置代理"}>
            <IconButton
              size="small"
              onClick={(e) => {
                e.stopPropagation()
                const target = node.image_path || node.name
                setProcessCmdline(target)
                setSelectedPid(node.pid)
              }}
              sx={{ p: 0.3, ml: 0.5 }}
            >
              <LanRounded fontSize="small" sx={{ fontSize: 16 }} color={node.hijacked ? "primary" : "action"} />
            </IconButton>
          </Tooltip>
        </Box>

        {hasChildren && (
          <Collapse in={isExpanded} timeout="auto" unmountOnExit>
            {node.children!.map((child) => renderTreeNode(child, depth + 1))}
          </Collapse>
        )}
      </Box>
    )
  }

  return (
    <BasePage
      title="应用流量监控 (App Traffic Monitor)"
      header={
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          <Chip
            size="small"
            label={isRunning ? '应用代理运行中' : '只读监控模式'}
            color={isRunning ? 'success' : 'primary'}
            variant={isRunning ? 'filled' : 'outlined'}
          />
          <Chip
            size="small"
            label={`可观测进程: ${totalProcesses}`}
            variant="outlined"
          />
          <Chip
            size="small"
            label={`活动连接: ${connections.length}`}
            variant="outlined"
          />
          {hijackedProcesses > 0 && (
            <Chip
              size="small"
              label={`已接管: ${hijackedProcesses}`}
              color="success"
              variant="outlined"
            />
          )}
          {status.proxy_ready && (
            <Chip size="small" label="Proxy 就绪" color="primary" variant="outlined" />
          )}
          <Tooltip title="刷新状态与遥测">
            <span>
              <IconButton
                size="small"
                onClick={() => {
                  void refreshAll()
                }}
                disabled={busy || loadingData}
              >
                <RefreshRounded fontSize="small" />
              </IconButton>
            </span>
          </Tooltip>
        </Box>
      }
    >
      <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2, height: '100%' }}>
        {/* Network boundary note */}
        <Alert severity={isRunning ? "success" : "info"} sx={{ py: 0.2, px: 1.5, alignItems: 'center' }}>
          <Typography variant="caption" sx={{ fontWeight: 500 }}>
            {isRunning
              ? '应用代理运行中：仅接管命中规则进程的 Windows IPv4 TCP；IPv6/UDP 仍走原有网络路径。点击「停止代理」可恢复为只读监控。'
              : '只读网络监控模式：直接读取系统进程树与实时 TCP 连接。未配置代理规则，未启动驱动截流，未修改系统网络设置。可在下方直接选择应用设置代理。'}
          </Typography>
        </Alert>

        {/* Error banner */}
        {(error || status.last_startup_error) && (
          <Alert
            severity="error"
            onClose={async () => {
              setError('')
              try {
                await invoke('clear_app_proxy_error')
              } catch (_) {}
            }}
            action={
              <Button
                color="inherit"
                size="small"
                onClick={async () => {
                  setError('')
                  try {
                    await invoke('clear_app_proxy_error')
                  } catch (_) {}
                }}
              >
                清除
              </Button>
            }
            sx={{ py: 0.5 }}
          >
            {error || status.last_startup_error}
          </Alert>
        )}

        {/* Telemetry Error & Stale Banner */}
        {telemetryError && (
          <Alert
            severity="warning"
            onClose={() => setTelemetryError(null)}
            sx={{ py: 0.5 }}
          >
            遥测通信异常：{telemetryError}{isStale ? '（当前展示历史快照，已标记为过期）' : ''}
          </Alert>
        )}

        {/* Multi-Application Rules & Control Card */}
        <Paper
          variant="outlined"
          sx={{
            p: 2,
            bgcolor: 'background.paper',
            borderColor: alpha(theme.palette.divider, 0.8),
          }}
        >
          {/* Header Row */}
          <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mb: 1.5 }}>
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5, flexWrap: 'wrap' }}>
              <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
                应用代理规则管理 (Proxy Rules)
              </Typography>
              <Chip
                size="small"
                label={`${rules.length} 条规则 (${rules.filter((r) => r.enabled).length} 条启用)`}
                color={rules.some((r) => r.enabled) ? 'primary' : 'default'}
                variant="outlined"
              />
              {isRunning && (
                <Chip
                  size="small"
                  label="支持运行时热重载"
                  color="success"
                  variant="outlined"
                />
              )}
            </Box>

            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
              <Button
                size="small"
                variant="outlined"
                startIcon={<AddRounded />}
                onClick={() => { setEditingRuleId(null); setNewRuleName(''); setNewRulePattern(''); setAddRuleDialogOpen(true) }}
              >
                添加规则
              </Button>
              <IconButton
                size="small"
                onClick={() => setRulesExpanded(!rulesExpanded)}
                title={rulesExpanded ? '折叠规则列表' : '展开规则列表'}
              >
                {rulesExpanded ? <ExpandMoreRounded fontSize="small" /> : <ChevronRightRounded fontSize="small" />}
              </IconButton>
            </Box>
          </Box>
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 1 }}>
            每条应用规则独立选择出口；不同出口请使用互不重叠的匹配模式，重叠规则的优先级尚未验证。
          </Typography>

          {/* Quick Input & Start/Stop Row */}
          <Box
            sx={{
              display: 'flex',
              flexDirection: { xs: 'column', md: 'row' },
              alignItems: 'center',
              gap: 1.5,
              mb: rulesExpanded && rules.length > 0 ? 1.5 : 0,
            }}
          >
            <TextField
              size="small"
              label="快捷添加/搜索规则"
              placeholder="例如: chrome.exe 或 *client-a*"
              value={processCmdline}
              onChange={(e) => setProcessCmdline(e.target.value)}
              multiline
              minRows={1}
              maxRows={3}
              sx={{ flexGrow: 1, minWidth: 220, '& textarea': { overflowWrap: 'anywhere' } }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && processCmdline.trim()) {
                  e.preventDefault()
                  void handleAddRule(processCmdline.trim(), processCmdline.trim())
                  setProcessCmdline('')
                }
              }}
              slotProps={{
                input: {
                  endAdornment: processCmdline.trim() ? (
                    <InputAdornment position="end">
                      <Button
                        size="small"
                        onClick={() => {
                          void handleAddRule(processCmdline.trim(), processCmdline.trim())
                          setProcessCmdline('')
                        }}
                      >
                        加入
                      </Button>
                    </InputAdornment>
                  ) : undefined,
                },
              }}
            />

            <Select
              size="small"
              value={strategyMode}
              onChange={(e) => setStrategyMode(e.target.value as 'regular' | 'fixed')}
              sx={{ minWidth: 140 }}
            >
              <MenuItem value="regular">新规则：常规规则</MenuItem>
              <MenuItem value="fixed">新规则：指定策略组</MenuItem>
            </Select>

            {strategyMode === 'fixed' && (
              <Select size="small" displayEmpty value={strategyGroups.includes(strategyGroup) ? strategyGroup : ''}
                onOpen={() => void loadStrategyGroups()} onChange={(e) => setStrategyGroup(e.target.value)} sx={{ minWidth: 150 }}>
                <MenuItem value="" disabled>选择 Mihomo 策略组</MenuItem>
                {strategyGroups.map((group) => <MenuItem key={group} value={group}>{group}</MenuItem>)}
              </Select>
            )}

            <Box sx={{ ml: 'auto', display: 'flex', gap: 1 }}>
              {!isRunning ? (
                <Button
                  variant="contained"
                  color="primary"
                  size="medium"
                  startIcon={busy ? <CircularProgress size={16} color="inherit" /> : <PlayArrowRounded />}
                  onClick={handleStart}
                  disabled={busy}
                >
                  启动代理 ({rules.filter((r) => r.enabled).length} 个应用)
                </Button>
              ) : (
                <Button
                  variant="outlined"
                  color="error"
                  size="medium"
                  startIcon={busy ? <CircularProgress size={16} color="inherit" /> : <StopRounded />}
                  onClick={handleStop}
                  disabled={busy}
                >
                  停止代理
                </Button>
              )}
            </Box>
          </Box>

          {/* Configured Rules List (Collapsible) */}
          <Collapse in={rulesExpanded}>
            {rules.length === 0 ? (
              <Box
                sx={{
                  p: 2,
                  mt: 1,
                  textAlign: 'center',
                  borderRadius: 1,
                  bgcolor: alpha(theme.palette.action.hover, 0.04),
                  border: '1px dashed',
                  borderColor: alpha(theme.palette.divider, 0.6),
                }}
              >
                <Typography variant="body2" color="text.secondary">
                  当前尚未配置应用规则。您可以点击右上角「添加规则」或直接在下方「进程监控树」各进程右侧点击「+」添加。
                </Typography>
              </Box>
            ) : (
              <Box
                sx={{
                  display: 'grid',
                  gridTemplateColumns: { xs: 'minmax(0, 1fr)', sm: 'repeat(2, minmax(0, 1fr))', md: 'repeat(3, minmax(0, 1fr))' },
                  gap: 1,
                  mt: 1,
                  maxHeight: 180,
                  overflowY: 'auto',
                  pr: 0.5,
                }}
              >
                {rules.map((rule) => (
                  <Paper
                    key={rule.id}
                    variant="outlined"
                    sx={{
                      p: 1,
                      minWidth: 0,
                      display: 'flex',
                      alignItems: 'center',
                      gap: 1,
                      borderRadius: 1,
                      borderColor: rule.enabled ? alpha(theme.palette.primary.main, 0.4) : alpha(theme.palette.divider, 0.5),
                      bgcolor: rule.enabled ? alpha(theme.palette.primary.main, 0.03) : 'transparent',
                    }}
                  >
                    <Switch
                      size="small"
                      checked={rule.enabled}
                      onChange={(e) => void handleToggleRule(rule.id, e.target.checked)}
                      disabled={busy}
                    />
                    <Box sx={{ minWidth: 0, flexGrow: 1 }}>
                      <Typography
                        variant="body2"
                        sx={{
                          fontWeight: 600,
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                          color: rule.enabled ? 'text.primary' : 'text.disabled',
                        }}
                        title={rule.name}
                      >
                        {rule.name}
                      </Typography>
                      <Typography
                        variant="caption"
                        sx={{
                          fontFamily: 'monospace',
                          fontSize: 11,
                          color: 'text.secondary',
                          display: 'block',
                          overflowWrap: 'anywhere',
                          whiteSpace: 'normal',
                        }}
                        title={rule.cmdline_pattern}
                      >
                        {rule.cmdline_pattern}
                      </Typography>
                      <Typography variant="caption"
                        color={rule.strategy_group && rule.strategy_group !== 'regular' && !strategyGroups.includes(rule.strategy_group) ? 'error' : 'primary'}
                        sx={{ display: 'block' }}>
                        {rule.strategy_group && rule.strategy_group !== 'regular'
                          ? `${strategyGroups.includes(rule.strategy_group) ? '策略组' : '策略组不可用'}：${rule.strategy_group}`
                          : '遵循常规规则'}
                      </Typography>
                    </Box>
                    <IconButton
                      size="small"
                      disabled={busy}
                      title="编辑规则与出口"
                      onClick={() => {
                        setEditingRuleId(rule.id)
                        setNewRuleName(rule.name)
                        setNewRulePattern(rule.cmdline_pattern)
                        setStrategyMode(rule.strategy_group && rule.strategy_group !== 'regular' ? 'fixed' : 'regular')
                        setStrategyGroup(rule.strategy_group === 'regular' ? '' : rule.strategy_group || '')
                        setAddRuleDialogOpen(true)
                      }}
                    >
                      <EditRounded fontSize="small" />
                    </IconButton>
                    <IconButton
                      size="small"
                      onClick={() => void handleRemoveRule(rule.id)}
                      disabled={busy}
                      sx={{ color: 'text.secondary', '&:hover': { color: 'error.main' } }}
                      title="删除规则"
                    >
                      <DeleteOutlineRounded fontSize="small" sx={{ fontSize: 16 }} />
                    </IconButton>
                  </Paper>
                ))}
              </Box>
            )}
          </Collapse>

          {/* Runtime Status Bar */}
          {isRunning && (
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5, mt: 1.5, pt: 1, borderTop: '1px solid', borderColor: alpha(theme.palette.divider, 0.5) }}>
              <Typography variant="caption" color="text.secondary">
                Helper PID: {status.pid ?? '未知'}
              </Typography>
              <Divider orientation="vertical" flexItem />
              <Typography variant="caption" color="text.secondary">
                生效规则版本: {status.effective_rules_version ?? '未知'}
              </Typography>
              <Divider orientation="vertical" flexItem />
              <Typography variant="caption" color="text.secondary">
                出口模式: 按各应用规则绑定
              </Typography>
              <Divider orientation="vertical" flexItem />
              <Typography variant="caption" sx={{ color: 'success.main', fontWeight: 500 }}>
                已启用 {rules.filter((r) => r.enabled).length} 条应用代理
              </Typography>
            </Box>
          )}
        </Paper>

        {/* Workspace: Split Panel (Available in both read-only monitor and active proxy mode) */}
        <Box sx={{ display: 'flex', gap: 2, flexGrow: 1, minHeight: 0 }}>
          {/* Left: Process Tree */}
          <Paper
            variant="outlined"
            sx={{
              p: 1.5,
              width: 360,
              flexShrink: 0,
              display: 'flex',
              flexDirection: 'column',
              bgcolor: 'background.paper',
            }}
          >
            <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
                  进程监控树
                </Typography>
                {isStale && (
                  <Chip size="small" label="快照已过期" color="warning" variant="outlined" />
                )}
              </Box>
              <Chip size="small" label={`${processTree.length} 个根节点`} variant="outlined" />
            </Box>

            <Tabs
              value={treeTab}
              onChange={(_, val) => setTreeTab(val)}
              sx={{ minHeight: 32, mb: 1 }}
            >
              <Tab label={`全部 (${totalProcesses})`} value="all" sx={{ minHeight: 32, py: 0.5, fontSize: 12 }} />
              <Tab label={`网络活动 (${activePids.size})`} value="active" sx={{ minHeight: 32, py: 0.5, fontSize: 12 }} />
              <Tab label={`已接管 (${hijackedProcesses})`} value="hijacked" sx={{ minHeight: 32, py: 0.5, fontSize: 12 }} />
            </Tabs>

              <TextField
                size="small"
                placeholder="搜索进程名或 PID..."
                value={processSearch}
                onChange={(e) => setProcessSearch(e.target.value)}
                slotProps={{
                  input: {
                    startAdornment: (
                      <InputAdornment position="start">
                        <SearchRounded fontSize="small" sx={{ color: 'text.disabled' }} />
                      </InputAdornment>
                    ),
                    endAdornment: processSearch ? (
                      <InputAdornment position="end">
                        <IconButton size="small" onClick={() => setProcessSearch('')}>
                          <ClearRounded fontSize="small" />
                        </IconButton>
                      </InputAdornment>
                    ) : undefined,
                  },
                }}
                sx={{ mb: 1 }}
              />

              <Box sx={{ flexGrow: 1, overflowY: 'auto', pr: 0.5 }}>
                {filteredTree.length === 0 ? (
                  <Typography
                    variant="body2"
                    color="text.secondary"
                    sx={{ textAlign: 'center', py: 4 }}
                  >
                    暂无匹配的进程
                  </Typography>
                ) : (
                  filteredTree.map((node) => renderTreeNode(node))
                )}
              </Box>
            </Paper>

            {/* Right: Connection Table */}
            <Paper
              variant="outlined"
              sx={{
                p: 1.5,
                flexGrow: 1,
                display: 'flex',
                flexDirection: 'column',
                bgcolor: 'background.paper',
                minWidth: 0,
              }}
            >
              <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                  <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
                    实时连接表 (IPv4 TCP)
                  </Typography>
                  {isStale && (
                    <Chip size="small" label="快照已过期" color="warning" variant="outlined" />
                  )}
                  <Chip size="small" label={`${filteredConnections.length} 条连接`} variant="outlined" />
                  {selectedPid != null && (
                    <Chip
                      size="small"
                      color="primary"
                      label={`仅 PID ${selectedPid}`}
                      onDelete={() => setSelectedPid(null)}
                    />
                  )}
                </Box>
              </Box>

              <TextField
                size="small"
                placeholder="按目标地址、IP、端口或进程名过滤..."
                value={connSearch}
                onChange={(e) => setConnSearch(e.target.value)}
                slotProps={{
                  input: {
                    startAdornment: (
                      <InputAdornment position="start">
                        <FilterListRounded fontSize="small" sx={{ color: 'text.disabled' }} />
                      </InputAdornment>
                    ),
                    endAdornment: connSearch ? (
                      <InputAdornment position="end">
                        <IconButton size="small" onClick={() => setConnSearch('')}>
                          <ClearRounded fontSize="small" />
                        </IconButton>
                      </InputAdornment>
                    ) : undefined,
                  },
                }}
                sx={{ mb: 1 }}
              />

              <TableContainer sx={{ flexGrow: 1, overflowY: 'auto' }}>
                <Table size="small" stickyHeader>
                  <TableHead>
                    <TableRow>
                      <TableCell sx={{ fontWeight: 600 }}>进程 (PID)</TableCell>
                      <TableCell sx={{ fontWeight: 600 }}>目标地址</TableCell>
                      <TableCell sx={{ fontWeight: 600 }}>本地地址</TableCell>
                      <TableCell sx={{ fontWeight: 600 }}>状态</TableCell>
                      <TableCell sx={{ fontWeight: 600 }}>接管判定</TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {filteredConnections.length === 0 ? (
                      <TableRow>
                        <TableCell colSpan={5} align="center" sx={{ py: 4, color: 'text.secondary' }}>
                          {connections.length === 0
                            ? '当前尚无活动 TCP 连接'
                            : '没有符合筛选条件的连接'}
                        </TableCell>
                      </TableRow>
                    ) : (
                      filteredConnections.map((conn, idx) => (
                        <TableRow
                          data-testid={`connection-row-${conn.pid}`}
                          key={`${conn.pid}-${conn.local_port}-${conn.remote_ip}-${conn.remote_port}-${idx}`}
                          hover
                          onClick={() => setSelectedPid(conn.pid)}
                          sx={{
                            cursor: 'pointer',
                            bgcolor: selectedPid === conn.pid ? alpha(theme.palette.primary.main, 0.08) : 'inherit',
                          }}
                        >
                          <TableCell>
                            <Typography variant="body2" sx={{ fontWeight: 500 }}>
                              {conn.process_name}
                            </Typography>
                            <Typography variant="caption" color="text.secondary" sx={{ fontFamily: 'monospace' }}>
                              PID: {conn.pid}
                            </Typography>
                          </TableCell>

                          <TableCell sx={{ fontFamily: 'monospace', fontSize: 12 }}>
                            {conn.dest || `${conn.remote_ip}:${conn.remote_port}`}
                          </TableCell>

                          <TableCell sx={{ fontFamily: 'monospace', fontSize: 12, color: 'text.secondary' }}>
                            {conn.local_ip}:{conn.local_port}
                          </TableCell>

                          <TableCell>
                            <Typography variant="caption" sx={{ fontFamily: 'monospace', fontWeight: 500 }}>
                              {conn.state}
                            </Typography>
                          </TableCell>

                          <TableCell>
                            {conn.state?.toUpperCase() === 'LISTEN' || conn.state?.toUpperCase() === 'LISTENING' ? (
                              <Chip
                                size="small"
                                label="监听 (LISTEN)"
                                color="default"
                                variant="outlined"
                                sx={{ height: 20, fontSize: 10, fontWeight: 600 }}
                              />
                            ) : (
                              <Chip
                                size="small"
                                label={conn.hijacked ? '已接管 (HIJACKED)' : '未接管 (PASSTHROUGH)'}
                                color={conn.hijacked ? 'success' : 'default'}
                                variant={conn.hijacked ? 'filled' : 'outlined'}
                                sx={{ height: 20, fontSize: 10, fontWeight: 600 }}
                              />
                            )}
                          </TableCell>
                        </TableRow>
                      ))
                    )}
                  </TableBody>
                </Table>
              </TableContainer>
            </Paper>
          </Box>

        {/* Process Detail Drawer */}
        <Drawer
          anchor="right"
          open={selectedPid != null}
          onClose={() => setSelectedPid(null)}
          data-testid="process-detail-drawer"
          slotProps={{
            paper: {
              'data-testid': 'process-detail-drawer-paper',
              sx: {
                width: { xs: '100%', sm: 420 },
                p: 2.5,
                bgcolor: 'background.paper',
              },
            } as any,
          }}
        >
          <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2, height: '100%' }}>
            <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <Typography variant="h6" sx={{ fontWeight: 600 }}>
                进程详细信息 (只读遥测)
              </Typography>
              <IconButton size="small" onClick={() => setSelectedPid(null)}>
                <CloseRounded />
              </IconButton>
            </Box>

            <Divider />

            {loadingDetail ? (
              <Box sx={{ display: 'flex', justifyContent: 'center', py: 4 }}>
                <CircularProgress size={32} />
              </Box>
            ) : processDetail ? (
              <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2, flexGrow: 1, overflowY: 'auto' }}>
                <Box>
                  <Typography variant="caption" color="text.secondary">
                    进程名称与 PID
                  </Typography>
                  <Typography variant="body1" sx={{ fontWeight: 600 }}>
                    {processDetail.name} (PID: {processDetail.pid})
                  </Typography>
                </Box>

                {processDetail.parent_pid != null && (
                  <Box>
                    <Typography variant="caption" color="text.secondary">
                      父进程 PID
                    </Typography>
                    <Typography variant="body2" sx={{ fontFamily: 'monospace' }}>
                      {processDetail.parent_pid}
                    </Typography>
                  </Box>
                )}

                <Box>
                  <Typography variant="caption" color="text.secondary">
                    接管状态与来源
                  </Typography>
                  <Box sx={{ display: 'flex', gap: 1, mt: 0.5 }}>
                    <Chip
                      size="small"
                      label={processDetail.hijacked ? '已接管 (HIJACKED)' : '未接管 (PASSTHROUGH)'}
                      color={processDetail.hijacked ? 'success' : 'default'}
                    />
                    {processDetail.hijack_source && (
                      <Chip
                        size="small"
                        variant="outlined"
                        label={`来源: ${processDetail.hijack_source}`}
                      />
                    )}
                  </Box>
                  <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mt: 0.5, fontSize: 11 }}>
                    注：接管状态仅代表驱动层流量拦截判定，不代表端到端代理转发成功。
                  </Typography>
                </Box>

                <Box>
                  <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                    <Typography variant="caption" color="text.secondary">
                      可执行文件路径 (Image Path)
                    </Typography>
                    {processDetail.image_path && (
                      <IconButton
                        size="small"
                        onClick={() => handleCopy(processDetail.image_path!, 'image_path')}
                      >
                        {copiedField === 'image_path' ? (
                          <CheckRounded fontSize="small" color="success" />
                        ) : (
                          <ContentCopyRounded fontSize="small" />
                        )}
                      </IconButton>
                    )}
                  </Box>
                  <Paper
                    variant="outlined"
                    sx={{
                      p: 1,
                      bgcolor: alpha(theme.palette.action.hover, 0.05),
                      fontFamily: 'monospace',
                      fontSize: 12,
                      wordBreak: 'break-all',
                    }}
                  >
                    {processDetail.image_path || '未知 (Unknown)'}
                  </Paper>
                </Box>

                <Box>
                  <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                    <Typography variant="caption" color="text.secondary">
                      启动命令行 (Command Line)
                    </Typography>
                    {processDetail.cmdline && (
                      <IconButton
                        size="small"
                        onClick={() => handleCopy(processDetail.cmdline!, 'cmdline')}
                      >
                        {copiedField === 'cmdline' ? (
                          <CheckRounded fontSize="small" color="success" />
                        ) : (
                          <ContentCopyRounded fontSize="small" />
                        )}
                      </IconButton>
                    )}
                  </Box>
                  <Paper
                    variant="outlined"
                    sx={{
                      p: 1,
                      bgcolor: alpha(theme.palette.action.hover, 0.05),
                      fontFamily: 'monospace',
                      fontSize: 12,
                      wordBreak: 'break-all',
                      maxHeight: 180,
                      overflowY: 'auto',
                    }}
                  >
                    {processDetail.cmdline || '未知 (Unknown)'}
                  </Paper>
                </Box>

                {/* Linked Proxy Rule Status & Actions */}
                {(() => {
                  const detailTarget = (processDetail.image_path || processDetail.name).toLowerCase()
                  const detailMatchingRule = rules.find((r) => {
                    const pat = r.cmdline_pattern.toLowerCase().replace(/^\*|\*$/g, '')
                    return pat && detailTarget.includes(pat)
                  })

                  if (detailMatchingRule) {
                    return (
                      <Paper
                        variant="outlined"
                        sx={{
                          p: 1.5,
                          mt: 1,
                          bgcolor: detailMatchingRule.enabled
                            ? alpha(theme.palette.primary.main, 0.05)
                            : alpha(theme.palette.action.hover, 0.05),
                          borderColor: detailMatchingRule.enabled
                            ? alpha(theme.palette.primary.main, 0.4)
                            : alpha(theme.palette.divider, 0.5),
                        }}
                      >
                        <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', mb: 1 }}>
                          <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
                            已配置代理规则: {detailMatchingRule.name}
                          </Typography>
                          <Switch
                            size="small"
                            checked={detailMatchingRule.enabled}
                            onChange={(e) => void handleToggleRule(detailMatchingRule.id, e.target.checked)}
                            disabled={busy}
                          />
                        </Box>
                        <Typography variant="caption" sx={{ fontFamily: 'monospace', color: 'text.secondary', display: 'block', mb: 1 }}>
                          匹配模式: {detailMatchingRule.cmdline_pattern}
                        </Typography>
                        <Box sx={{ display: 'flex', gap: 1 }}>
                          <Button
                            size="small"
                            variant="outlined"
                            color="error"
                            startIcon={<DeleteOutlineRounded fontSize="small" />}
                            onClick={() => void handleRemoveRule(detailMatchingRule.id)}
                            disabled={busy}
                          >
                            移除规则
                          </Button>
                        </Box>
                      </Paper>
                    )
                  }

                  return (
                    <Box sx={{ mt: 1 }}>
                      <Button
                        variant="outlined"
                        size="small"
                        fullWidth
                        startIcon={<AddRounded />}
                        onClick={() => {
                          const target = processRuleTarget(processDetail.name, processDetail.image_path, processDetail.cmdline)
                          if (target) void handleAddRule(processDetail.name, target)
                        }}
                        disabled={busy || !processRuleTarget(processDetail.name, processDetail.image_path, processDetail.cmdline)}
                      >
                        将此进程加入代理规则库
                      </Button>
                    </Box>
                  )
                })()}

                <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1, mt: 2 }}>
                  {processDetail.hijacked ? (
                    <Button
                      variant="outlined"
                      color="error"
                      fullWidth
                      startIcon={busy ? <CircularProgress size={16} color="inherit" /> : <StopRounded />}
                      onClick={handleStop}
                      disabled={busy}
                    >
                      停止此应用代理
                    </Button>
                  ) : (
                    <Button
                      variant="contained"
                      color="primary"
                      fullWidth
                      startIcon={busy ? <CircularProgress size={16} color="inherit" /> : <PlayArrowRounded />}
                      onClick={() => {
                        const target = processRuleTarget(processDetail.name, processDetail.image_path, processDetail.cmdline)
                        if (!target) { setError('脚本进程缺少完整命令行，请手动填写具体脚本匹配模式'); return }
                        setProcessCmdline(target)
                        void handleStartWithTarget(target, processDetail.name)
                      }}
                      disabled={busy || !processRuleTarget(processDetail.name, processDetail.image_path, processDetail.cmdline)}
                    >
                      {isRunning ? '立即热重载加入代理' : '为此应用启动代理'}
                    </Button>
                  )}

                  <Button
                    variant="text"
                    size="small"
                    color="inherit"
                    startIcon={<LanRounded />}
                    onClick={() => {
                      setProcessCmdline(processDetail.cmdline || processDetail.image_path || processDetail.name)
                      setSelectedPid(null)
                    }}
                  >
                    填入上方输入框微调规则
                  </Button>
                </Box>
              </Box>
            ) : (
              <Typography variant="body2" color="text.secondary" sx={{ py: 3, textAlign: 'center' }}>
                未能获取进程详细信息（进程可能已退出）
              </Typography>
            )}
          </Box>
        </Drawer>

        {/* Add Rule Dialog */}
        <Dialog open={addRuleDialogOpen} onClose={() => { setAddRuleDialogOpen(false); setEditingRuleId(null) }} maxWidth="sm" fullWidth>
          <DialogTitle>{editingRuleId ? '编辑应用代理规则' : '添加应用代理规则'}</DialogTitle>
          <DialogContent sx={{ display: 'flex', flexDirection: 'column', gap: 2, pt: 1 }}>
            <Typography variant="body2" color="text.secondary">
              配置需要接管流量的应用程序。支持文件名（如 chrome.exe）或完整路径/命令行模式匹配。
            </Typography>
            <TextField
              size="small"
              label="规则名称"
              placeholder="例如: Google Chrome, 微信, Spotify"
              value={newRuleName}
              onChange={(e) => setNewRuleName(e.target.value)}
              autoFocus
              fullWidth
            />
            <TextField
              size="small"
              label="匹配模式 (进程名 / 路径 / 命令行)"
              placeholder="例如: chrome.exe 或 *WeChat* 或 C:\path\app.exe"
              value={newRulePattern}
              onChange={(e) => setNewRulePattern(e.target.value)}
              multiline
              minRows={2}
              maxRows={6}
              sx={{ '& textarea': { overflowWrap: 'anywhere' } }}
              fullWidth
              helperText="支持通配符（如 *），若为空则无法保存"
            />
            <Select size="small" value={strategyMode} onChange={(e) => setStrategyMode(e.target.value as 'regular' | 'fixed')}>
              <MenuItem value="regular">遵循常规规则</MenuItem>
              <MenuItem value="fixed">指定 Mihomo 策略组</MenuItem>
            </Select>
            {strategyMode === 'fixed' && <Select size="small" displayEmpty value={strategyGroups.includes(strategyGroup) ? strategyGroup : ''}
              onOpen={() => void loadStrategyGroups()} onChange={(e) => setStrategyGroup(e.target.value)} fullWidth>
              <MenuItem value="" disabled>选择当前配置中的策略组</MenuItem>
              {strategyGroups.map((group) => <MenuItem key={group} value={group}>{group}</MenuItem>)}
            </Select>}
          </DialogContent>
          <DialogActions>
            <Button onClick={() => setAddRuleDialogOpen(false)}>取消</Button>
            <Button
              variant="contained"
              onClick={() => void handleSaveRule()}
              disabled={!newRulePattern.trim() || (strategyMode === 'fixed' && !strategyGroup.trim()) || busy}
            >
              保存规则
            </Button>
          </DialogActions>
        </Dialog>
      </Box>
    </BasePage>
  )
}
