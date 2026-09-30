import { describe, expect, it } from 'vitest'

import { resolveProxyObservation } from './system-proxy-observation'

const input = {
  automatic: false,
  host: '127.0.0.1',
  mixedPort: 7897,
}

describe('system proxy observation', () => {
  it('does not report a failed read as an OS disable', () => {
    expect(resolveProxyObservation(input)).toBe('unknown')
  })

  it('distinguishes an active other endpoint from a disabled proxy', () => {
    expect(resolveProxyObservation({
      ...input,
      systemProxy: { enable: true, server: '127.0.0.1:9999' },
    })).toBe('different')
    expect(resolveProxyObservation({
      ...input,
      systemProxy: { enable: false, server: '127.0.0.1:7897' },
    })).toBe('disabled')
  })

  it('reports the expected endpoint as enabled', () => {
    expect(resolveProxyObservation({
      ...input,
      systemProxy: { enable: true, server: '127.0.0.1:7897' },
    })).toBe('enabled')
  })
})
