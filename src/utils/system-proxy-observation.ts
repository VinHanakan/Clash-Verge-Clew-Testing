export type ProxyObservation =
  | 'enabled'
  | 'disabled'
  | 'different'
  | 'unknown'

interface ProxyObservationInput {
  automatic: boolean
  autoProxy?: { enable: boolean; url: string }
  systemProxy?: { enable: boolean; server: string }
  pacPort?: number
  host: string
  mixedPort: number
}

export const resolveProxyObservation = ({
  automatic,
  autoProxy,
  systemProxy,
  pacPort,
  host,
  mixedPort,
}: ProxyObservationInput): ProxyObservation => {
  if (automatic) {
    if (!autoProxy) return 'unknown'
    if (!autoProxy.enable) return 'disabled'
    if (!pacPort) return 'unknown'
    return autoProxy.url === `http://${host}:${pacPort}/commands/pac`
      ? 'enabled'
      : 'different'
  }

  if (!systemProxy) return 'unknown'
  if (!systemProxy.enable) return 'disabled'
  return systemProxy.server === `${host}:${mixedPort}`
    ? 'enabled'
    : 'different'
}
